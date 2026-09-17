local M = {}

local function reject_duplicate_fields(encoded)
  local length = #encoded
  local function skip_space(index)
    while index <= length and encoded:sub(index, index):match "%s" do
      index = index + 1
    end
    return index
  end

  local function parse_string(index)
    if encoded:sub(index, index) ~= '"' then
      return nil
    end
    local start = index
    index = index + 1
    while index <= length do
      local character = encoded:sub(index, index)
      if character == '"' then
        return index + 1, encoded:sub(start, index)
      end
      if character == "\\" then
        index = index + 1
        if index > length then
          return nil
        end
        if encoded:sub(index, index) == "u" then
          if not encoded:sub(index + 1, index + 4):match "^%x%x%x%x$" then
            return nil
          end
          index = index + 4
        end
      end
      index = index + 1
    end
    return nil
  end

  local parse_value
  parse_value = function(index)
    index = skip_space(index)
    local character = encoded:sub(index, index)
    if character == '"' then
      return parse_string(index)
    end
    if character == "{" then
      local seen = {}
      index = skip_space(index + 1)
      if encoded:sub(index, index) == "}" then
        return index + 1
      end
      while index <= length do
        local next_index, raw_key = parse_string(index)
        if not next_index then
          return nil
        end
        local decoded, key = pcall(vim.json.decode, raw_key)
        key = decoded and key or raw_key
        if seen[key] then
          return nil
        end
        seen[key] = true
        index = skip_space(next_index)
        if encoded:sub(index, index) ~= ":" then
          return nil
        end
        index = parse_value(index + 1)
        if not index then
          return nil
        end
        index = skip_space(index)
        local delimiter = encoded:sub(index, index)
        if delimiter == "}" then
          return index + 1
        end
        if delimiter ~= "," then
          return nil
        end
        index = skip_space(index + 1)
      end
      return nil
    end
    if character == "[" then
      index = skip_space(index + 1)
      if encoded:sub(index, index) == "]" then
        return index + 1
      end
      while index <= length do
        index = parse_value(index)
        if not index then
          return nil
        end
        index = skip_space(index)
        local delimiter = encoded:sub(index, index)
        if delimiter == "]" then
          return index + 1
        end
        if delimiter ~= "," then
          return nil
        end
        index = skip_space(index + 1)
      end
      return nil
    end
    local start = index
    while index <= length and not encoded:sub(index, index):match "[%s,%]}]" do
      index = index + 1
    end
    return index > start and index or nil
  end

  local final = parse_value(1)
  return final ~= nil and skip_space(final) == length + 1
end

function M.decode(encoded)
  if type(encoded) ~= "string" or encoded == "" or not reject_duplicate_fields(encoded) then
    return nil
  end
  local decoded, value = pcall(vim.json.decode, encoded)
  return decoded and value or nil
end

return M
