-- Execute generated client configuration without SSH, filesystem writes,
-- terminal processes, or a GUI. Print captured data as independent UTF-8 hex.
local path, mode = arg[1], arg[2]
local chunk = assert(loadfile(path))
local writes = {}
local stages = {}
local last
local spawned
local panes = {}
local callbacks = {}
local scheduled = {}
local domain = (mode == "local-spawn" or mode == "spawn-error") and "local" or "remote-test"
local function output(name, value)
  local hex = tostring(value):gsub(".", function(byte)
    return string.format("%02x", string.byte(byte))
  end)
  io.stdout:write(name .. "=" .. hex .. "\n")
end
local function pane(id)
  return {
    pane_id = function()
      return id
    end,
    get_domain_name = function()
      return domain
    end,
    get_user_vars = function()
      return {
        WISP_TEST_ATTACHMENT = "owned-cookie",
        WISP_TEST_ATTACHMENT_ACK = "ack",
        WISP_TEST_RAW_ACK = "diagnostic-only",
        PRIVATE_FIXTURE = "do-not-record",
      }
    end,
  }
end
local wezterm = {
  on = function(name, callback)
    callbacks[name] = callback
  end,
  json_encode = function(snapshot)
    assert(snapshot.probe_version == 1, "snapshot version")
    last = snapshot
    if stages[#stages] ~= snapshot.stage then
      table.insert(stages, snapshot.stage)
    end
    return "{}"
  end,
  time = {
    call_after = function(_, callback)
      table.insert(scheduled, callback)
    end,
  },
  mux = {
    get_domain = function(name)
      assert(name == domain, "attachment domain")
      return {
        attach = function()
          if mode == "attach-error" then
            error "simulated attachment failure"
          end
          table.insert(panes, pane(7))
        end,
        detach = function()
          assert(mode == "detach", "only the owned detach test may detach")
          panes = {}
        end,
      }
    end,
    spawn_window = function(request)
      assert(#panes == 1, "spawn must follow successful attachment")
      if mode == "spawn-error" then
        error "simulated spawn failure with environment FIXTURE_SECRET=do-not-record"
      end
      spawned = request
      local created = pane(19)
      table.insert(panes, created)
      return {}, created, {}
    end,
    all_windows = function()
      return {
        {
          tabs = function()
            return {
              {
                panes = function()
                  return panes
                end,
              },
            }
          end,
        },
      }
    end,
  },
}
package.preload.wezterm = function()
  return wezterm
end
local real_open, real_rename = io.open, os.rename
io.open = function(name, access)
  if access == "r" then
    assert(mode == "detach", "only an explicit detach signal may be read")
    return { close = function() end }
  end
  assert(access == "w", "only snapshot writes are allowed")
  return {
    write = function(_, bytes)
      writes[name] = bytes
    end,
    close = function() end,
  }
end
os.rename = function(source, destination)
  assert(writes[source], "atomic snapshot source must exist")
  writes[destination], writes[source] = writes[source], nil
  return true
end
local ok, failure = pcall(function()
  local config = chunk()
  assert(config.automatically_reload_config == false, "immutable client config must not reload on state-file writes")
  if config.unix_domains[2] then
    assert(#config.ssh_domains == 0, "argv proxy cannot invoke native SSH")
    local proxy = config.unix_domains[2]
    assert(proxy.name == "remote-test" and proxy.no_serve_automatically, "owned proxy domain")
    output("proxy_socket", proxy.socket_path)
    for _, value in ipairs(proxy.proxy_command) do
      output("proxy_arg", value)
    end
  else
    assert(config.ssh_domains[1].name == "remote-test", "SSH domain policy")
    assert(not config.ssh_domains[1].connect_automatically, "no implicit SSH connection")
    output("ssh_verbose", config.ssh_domains[1].ssh_option.wezterm_ssh_verbose)
    output("ssh_address", config.ssh_domains[1].remote_address)
  end
  callbacks["mux-startup"]()
  assert(#scheduled == 1, "one snapshot timer")
  scheduled[1]()
end)
io.open, os.rename = real_open, real_rename
assert(ok, failure)
for _, stage in ipairs(stages) do
  output("stage", stage)
end
if spawned then
  output("domain", spawned.domain.DomainName)
  output("cwd", spawned.cwd)
  for _, value in ipairs(spawned.args) do
    output("arg", value)
  end
end
output("marker", last.marker)
output("generation", last.generation)
output("transport", last.transport)
if mode == "private-data" then
  output("private", last.panes[1].vars.PRIVATE_FIXTURE)
end
if mode == "raw-diagnostic" then
  output("raw", last.panes[1].vars.WISP_TEST_RAW_ACK)
end
if last.spawned_pane_id then
  output("spawned_pane_id", last.spawned_pane_id)
end
if last.error then
  output("error", last.error)
end
