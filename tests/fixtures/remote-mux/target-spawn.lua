-- Target-native structured spawn handling for the owned feasibility fixture.
local wezterm, root, marker, node = ...
local consumed = false
local function read_json(path)
  local file = io.open(path, "r")
  if not file then
    return nil
  end
  local source = file:read(4097)
  file:close()
  assert(source and #source <= 4096, "oversized target request")
  local value = wezterm.json_parse(source)
  assert(type(value) == "table" and value.probe_version == 1, "unsupported target version")
  return value
end
local function publish(name, value)
  local path = root .. "/spawn." .. name .. ".json"
  local file = assert(io.open(path .. ".tmp", "w"))
  file:write(wezterm.json_encode(value))
  file:close()
  assert(os.rename(path .. ".tmp", path))
end
local function native_path(path)
  if type(path) ~= "string" or path:find "%z" then
    return false
  end
  if wezterm.target_triple:find "windows" then
    return path:match "^[A-Za-z]:[/\\]" or path:match "^\\\\[^\\]+\\[^\\]+"
  end
  return path:sub(1, 1) == "/"
end
local function execute()
  if read_json(root .. "/spawn.claim.json") then
    consumed = true
    return
  end
  local command = read_json(root .. "/spawn.request.json")
  if not command then
    return
  end
  consumed = true
  assert(command.action == "spawn" and command.marker == marker, "foreign target request")
  assert(
    type(command.generation) == "string" and #command.generation == 24 and command.generation:match "^[a-f0-9]+$",
    "invalid generation"
  )
  assert(type(command.cookie) == "string" and #command.cookie > 0, "invalid cookie")
  assert(
    type(command.sequence) == "number"
      and command.sequence >= 1
      and command.sequence <= 9007199254740991
      and command.sequence % 1 == 0,
    "invalid sequence"
  )
  assert(
    native_path(command.cwd) and type(command.args) == "table" and #command.args >= 4 and #command.args <= 128,
    "invalid native command"
  )
  for key, value in pairs(command.args) do
    assert(type(key) == "number" and key % 1 == 0 and key >= 1 and key <= #command.args, "invalid argv index")
    assert(type(value) == "string" and not value:find "%z", "invalid argv byte")
  end
  assert(
    command.args[1] == node
      and command.args[2] == root .. "/probe.mjs"
      and command.args[3] == root .. "/argv.json"
      and command.args[4] == root .. "/stop",
    "foreign fixture command"
  )
  local matches = 0
  for _, window in ipairs(wezterm.mux.all_windows()) do
    for _, tab in ipairs(window:tabs()) do
      for _, pane in ipairs(tab:panes()) do
        local vars = pane:get_user_vars() or {}
        if
          pane:get_domain_name() == "local"
          and vars.WISP_TEST_ATTACHMENT == command.cookie
          and vars.WISP_TEST_ATTACHMENT_ACK
        then
          local ack = wezterm.json_parse(vars.WISP_TEST_ATTACHMENT_ACK)
          assert(type(ack) == "table" and ack.probe_version == 1, "unsupported keeper acknowledgement")
          if
            ack.action == "publish"
            and ack.status == "published"
            and ack.marker == marker
            and ack.generation == command.generation
            and ack.cookie == command.cookie
            and ack.sequence == command.sequence
          then
            matches = matches + 1
          end
        end
      end
    end
  end
  assert(matches == 1, "missing or ambiguous keeper acknowledgement")
  local result = {
    probe_version = 1,
    marker = marker,
    generation = command.generation,
    cookie = command.cookie,
    sequence = command.sequence,
  }
  -- A durable claim precedes CommandBuilder creation. Reloads and repeated
  -- requests cannot launch another process, even if the previous spawn failed.
  publish("claim", result)
  local _, pane = wezterm.mux.spawn_window { args = command.args, cwd = command.cwd, domain = { DomainName = "local" } }
  result.stage, result.pane_id = "ready", pane:pane_id()
  publish("result", result)
end
local function tick()
  if consumed then
    return
  end
  local ok = pcall(execute)
  if not ok then
    consumed = true
    -- Native errors may contain a full command/environment Debug dump.
    publish(
      "result",
      { probe_version = 1, marker = marker, stage = "failed", error = "target spawn rejected or failed" }
    )
  end
  if not consumed then
    wezterm.time.call_after(0.1, tick)
  end
end
wezterm.on("mux-startup", function()
  wezterm.time.call_after(0.1, tick)
end)
