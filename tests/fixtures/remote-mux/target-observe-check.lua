local helper, data_file = arg[1], arg[2]
local data = assert(loadfile(data_file))()
local files, values, results, timers, callbacks = {}, {}, {}, {}, {}
local root, current, encoded = data.root, data.before, 0
local pane = {
  pane_id = function()
    return 7
  end,
  get_domain_name = function()
    return "local"
  end,
  get_user_vars = function()
    return current
  end,
}
local wezterm = {
  on = function(name, callback)
    callbacks[name] = callback
  end,
  time = {
    call_after = function(_, callback)
      timers[#timers + 1] = callback
    end,
  },
  json_parse = function(source)
    if source == "request" and data.mode == "wrong-version" then
      return setmetatable({ probe_version = 2 }, {
        __index = function()
          error "read incompatible observation"
        end,
      })
    end
    return values[source]
  end,
  json_encode = function(value)
    encoded = encoded + 1
    local key = "json" .. encoded
    values[key] = value
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
                  return { pane }
                end,
              },
            }
          end,
        },
      }
    end,
    spawn_window = function()
      error "diagnostic observation must not spawn"
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
  assert(mode == "w")
  return {
    close = function() end,
    write = function(_, source)
      files[path] = source
    end,
  }
end
os.rename = function(source, destination)
  assert(files[source] and not files[destination], "publish each Windows diagnostic file only once")
  files[destination], files[source] = files[source], nil
  results[destination] = values[files[destination]]
  return true
end
local ok, failure = pcall(function()
  assert(loadfile(helper))(wezterm, root, data.marker)
  assert(callbacks["mux-startup"], "observer must register mux-startup")
  callbacks["mux-startup"]()
  values.request = data.request
  files[root .. "/observation.request.json"] = data.mode == "oversized" and string.rep("x", 4097) or "request"
  assert(timers[1], "observer must poll without spawning")()
  if timers[2] then
    current = data.after
    values.request = data.next_request
    timers[2]()
  end
end)
io.open, os.rename = real_open, real_rename
assert(ok, failure)
local function output(name, value)
  io.stdout:write(name .. "=" .. tostring(value):gsub(".", function(byte)
    return string.format("%02x", byte:byte())
  end) .. "\n")
end
for _, phase in ipairs { "startup", "timer", "before", "after", "error" } do
  local result = results[root .. "/observation." .. phase .. ".json"]
  if result then
    output(phase .. "_stage", result.stage)
    output(phase .. "_version", result.probe_version)
    output(phase .. "_private_request", result.PRIVATE_FIXTURE)
    for _, p in ipairs(result.panes or {}) do
      output(phase .. "_pane", p.pane_id)
      output(phase .. "_token", p.vars.WISP_TEST_ATTACHMENT)
      output(phase .. "_raw", p.vars.WISP_TEST_RAW_ACK)
      output(phase .. "_private", p.vars.PRIVATE_FIXTURE)
    end
  end
end
