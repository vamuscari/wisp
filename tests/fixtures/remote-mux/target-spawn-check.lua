-- Execute the target helper with fake mux/filesystem APIs; never spawn or write.
local helper, data_file = arg[1], arg[2]
local data = assert(loadfile(data_file))()
local files, encoded, callbacks, scheduled = {}, {}, {}, {}
local root, marker, node = data.root, data.marker, data.node
local last, spawned, calls = nil, nil, 0
if data.mode ~= "missing-request" then
  files[root .. "/spawn.request.json"] = data.mode == "oversized" and string.rep("x", 4097) or "request"
end
if data.mode == "claimed" then
  files[root .. "/spawn.claim.json"] = "claim"
end
local pane = {
  get_domain_name = function()
    return "local"
  end,
  get_user_vars = function()
    if data.mode == "foreign-server" then
      return {}
    end
    return { WISP_TEST_ATTACHMENT = data.request.cookie, WISP_TEST_ATTACHMENT_ACK = "ack" }
  end,
}
local wezterm = {
  target_triple = "x86_64-pc-windows-msvc",
  on = function(name, fn)
    callbacks[name] = fn
  end,
  time = {
    call_after = function(_, fn)
      scheduled[#scheduled + 1] = fn
    end,
  },
  json_parse = function(source)
    if encoded[source] then
      return encoded[source]
    end
    if source == "request" then
      if data.mode == "wrong-version" then
        return setmetatable({ probe_version = 2 }, {
          __index = function()
            error "interpreted incompatible request"
          end,
        })
      end
      return data.request
    elseif source == "ack" then
      if data.mode == "wrong-ack-version" then
        return { probe_version = 2 }
      end
      return data.ack
    elseif source == "claim" then
      return { probe_version = 1, marker = marker }
    end
    error "unexpected JSON input"
  end,
  json_encode = function(value)
    local key = "encoded" .. tostring(#encoded + 1)
    -- String keys are deliberately unique even when a claim and result coexist.
    while encoded[key] do
      key = key .. "x"
    end
    encoded[key] = value
    last = value
    return key
  end,
  mux = {
    all_windows = function()
      return {
        {
          tabs = function()
            return {
              {
                panes = function()
                  return data.mode == "duplicate-keeper" and { pane, pane } or { pane }
                end,
              },
            }
          end,
        },
      }
    end,
    spawn_window = function(request)
      assert(files[root .. "/spawn.claim.json"], "claim must precede the native spawn")
      calls = calls + 1
      if data.mode == "spawn-error" then
        error "CommandBuilder FIXTURE_SECRET=do-not-record"
      end
      spawned = request
      return {}, {
        pane_id = function()
          return 19
        end,
      }, {}
    end,
  },
}
local real_open, real_rename = io.open, os.rename
io.open = function(path, mode)
  if mode == "r" then
    if not files[path] then
      return nil
    end
    return {
      close = function() end,
      read = function(_, limit)
        return files[path]:sub(1, limit)
      end,
    }
  end
  assert(mode == "w", "only fixed helper writes are allowed")
  return {
    close = function() end,
    write = function(_, value)
      files[path] = value
    end,
  }
end
os.rename = function(source, target)
  assert(files[source], "atomic source is required")
  assert(not files[target], "Windows rename cannot overwrite a published result/claim")
  files[target], files[source] = files[source], nil
  return true
end
local ok, failure = pcall(function()
  assert(loadfile(helper))(wezterm, root, marker, node)
  assert(callbacks["mux-startup"], "target helper must register startup polling")
  callbacks["mux-startup"]()
  scheduled[1]()
  if scheduled[2] then
    scheduled[2]()
  end
end)
io.open, os.rename = real_open, real_rename
assert(ok, failure)
local function output(name, value)
  io.stdout:write(name .. "=" .. tostring(value):gsub(".", function(byte)
    return string.format("%02x", byte:byte())
  end) .. "\n")
end
output("calls", calls)
if last then
  output("stage", last.stage)
  output("error", last.error)
  output("pane_id", last.pane_id)
end
if spawned then
  output("domain", spawned.domain.DomainName)
  output("cwd", spawned.cwd)
  for _, value in ipairs(spawned.args) do
    output("arg", value)
  end
end
