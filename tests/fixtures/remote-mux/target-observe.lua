-- Diagnostic observation only; never authorizes identity or starts a process.
local wezterm, root, marker = ...
local observed, failed = {}, false
local function read_json(path)
  local file = io.open(path, "r")
  if not file then
    return nil
  end
  local source = file:read(4097)
  file:close()
  assert(source and #source <= 4096, "oversized observation")
  local value = wezterm.json_parse(source)
  assert(type(value) == "table" and value.probe_version == 1, "unsupported observation version")
  return value
end
local function publish(phase, value)
  local source = wezterm.json_encode(value)
  assert(#source <= 4096, "oversized observation result")
  local path = root .. "/observation." .. phase .. ".json"
  local out = assert(io.open(path .. ".tmp", "w"))
  out:write(source)
  out:close()
  assert(os.rename(path .. ".tmp", path))
end
local function milestone(phase, stage)
  local existing = read_json(root .. "/observation." .. phase .. ".json")
  if existing then
    assert(existing.marker == marker, "foreign observer milestone")
    return
  end
  publish(phase, { probe_version = 1, marker = marker, stage = stage })
end
local function inspect()
  local request = read_json(root .. "/observation.request.json")
  if not request then
    return
  end
  assert(request.action == "observe" and request.marker == marker, "foreign observation")
  assert(request.phase == "before" or request.phase == "after", "unknown observation phase")
  assert(
    type(request.generation) == "string" and #request.generation == 24 and request.generation:match "^[a-f0-9]+$",
    "invalid observation generation"
  )
  assert(type(request.cookie) == "string" and #request.cookie > 0, "invalid observation cookie")
  assert(
    type(request.sequence) == "number" and request.sequence >= 1 and request.sequence % 1 == 0,
    "invalid observation sequence"
  )
  if observed[request.phase] then
    return
  end
  local path = root .. "/observation." .. request.phase .. ".json"
  local existing = read_json(path)
  if existing then
    assert(existing.marker == marker and existing.generation == request.generation, "foreign observation result")
    observed[request.phase] = true
    return
  end
  local panes = {}
  for _, window in ipairs(wezterm.mux.all_windows()) do
    for _, tab in ipairs(window:tabs()) do
      for _, pane in ipairs(tab:panes()) do
        if pane:get_domain_name() == "local" then
          assert(#panes < 2, "too many local observation panes")
          local source = pane:get_user_vars() or {}
          local vars = {}
          for _, name in ipairs {
            "WISP_TEST_ATTACHMENT",
            "WISP_TEST_ATTACHMENT_ACK",
            "WISP_TEST_BARRIER",
            "WISP_TEST_RAW_ACK",
          } do
            if source[name] then
              assert(type(source[name]) == "string" and #source[name] <= 1024, "oversized observation variable")
              vars[name] = source[name]
            end
          end
          table.insert(panes, { pane_id = pane:pane_id(), domain = "local", vars = vars })
        end
      end
    end
  end
  publish(request.phase, {
    probe_version = 1,
    action = "observe",
    marker = marker,
    generation = request.generation,
    cookie = request.cookie,
    sequence = request.sequence,
    phase = request.phase,
    stage = "observed",
    panes = panes,
  })
  observed[request.phase] = true
end
local function tick()
  if failed or observed.before and observed.after then
    return
  end
  if not pcall(function()
    milestone("timer", "ticking")
    inspect()
  end) then
    failed = true
    publish("error", { probe_version = 1, marker = marker, stage = "failed", error = "target observation failed" })
    return
  end
  wezterm.time.call_after(0.1, tick)
end
wezterm.on("mux-startup", function()
  milestone("startup", "started")
  wezterm.time.call_after(0.1, tick)
end)
