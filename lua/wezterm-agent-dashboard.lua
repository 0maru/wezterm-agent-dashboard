-- wezterm-agent-dashboard.lua
-- WezTerm Lua module for agent dashboard integration.
--
-- Provides:
--   1. Status bar summary showing agent counts by status
--   2. Keybinding to toggle the dashboard sidebar
--
-- Usage in wezterm.lua:
--   local dashboard = require("wezterm-agent-dashboard")
--   dashboard.setup({ toggle_key = { key = "e", mods = "LEADER" } })
--   dashboard.apply_to_config(config)

local wezterm = require("wezterm")
local M = {}

-- ---------------------------------------------------------------------------
-- Configuration
-- ---------------------------------------------------------------------------

M.config = {
    toggle_key = { key = "e", mods = "LEADER" },
    sidebar_percent = 20,
    sidebar_position = "Right",
    show_status_bar = true,
    binary_name = "wezterm-agent-dashboard",
}

function M.setup(user_config)
    if user_config then
        for k, v in pairs(user_config) do
            M.config[k] = v
        end
    end
end

-- ---------------------------------------------------------------------------
-- Binary resolution
-- ---------------------------------------------------------------------------

local function find_binary()
    local candidates = {
        M.config.binary_name,
        wezterm.home_dir .. "/.local/bin/wezterm-agent-dashboard",
        wezterm.home_dir .. "/.cargo/bin/wezterm-agent-dashboard",
    }

    for _, path in ipairs(candidates) do
        -- Check if the file exists and is executable
        local f = io.open(path, "r")
        if f then
            f:close()
            return path
        end
    end

    -- Fallback: assume it's in PATH
    return M.config.binary_name
end

-- ---------------------------------------------------------------------------
-- Status bar: agent summary
-- ---------------------------------------------------------------------------

local status_icons = {
    running = { icon = "●", color = "#87d787" },   -- green
    waiting = { icon = "◐", color = "#ffd75f" },   -- yellow
    idle    = { icon = "○", color = "#87afaf" },    -- teal
    error   = { icon = "✕", color = "#ff5f5f" },   -- red
}

local function count_agents()
    local counts = { running = 0, waiting = 0, idle = 0, error = 0 }
    local total = 0

    -- Use wezterm.mux to enumerate all panes
    local success, all_panes = pcall(function()
        return wezterm.mux.all_panes()
    end)

    if not success or not all_panes then
        return counts, total
    end

    for _, pane in ipairs(all_panes) do
        local ok, vars = pcall(function()
            return pane:get_user_vars()
        end)

        if ok and vars and vars.agent_status then
            local status = vars.agent_status
            if counts[status] ~= nil then
                counts[status] = counts[status] + 1
                total = total + 1
            end
        end
    end

    return counts, total
end

local function build_status_bar()
    local counts, total = count_agents()
    if total == 0 then
        return ""
    end

    local parts = {}

    for _, key in ipairs({ "running", "waiting", "idle", "error" }) do
        if counts[key] > 0 then
            local info = status_icons[key]
            table.insert(parts, wezterm.format({
                { Foreground = { Color = info.color } },
                { Text = info.icon .. " " .. tostring(counts[key]) },
            }))
        end
    end

    if #parts == 0 then
        return ""
    end

    return table.concat(parts, " ")
end

-- ---------------------------------------------------------------------------
-- Toggle action
-- ---------------------------------------------------------------------------

local function create_toggle_action()
    return wezterm.action_callback(function(window, pane)
        local bin = find_binary()

        -- Check if dashboard pane exists in this tab
        local tab = pane:tab()
        if tab then
            local panes_with_info = tab:panes_with_info()
            for _, p in ipairs(panes_with_info) do
                local ok, vars = pcall(function()
                    return p.pane:get_user_vars()
                end)
                if ok and vars and vars.pane_role == "dashboard" then
                    -- Dashboard exists, kill it
                    -- Use the CLI to kill the pane
                    local pane_id = p.pane:pane_id()
                    wezterm.run_child_process({
                        "wezterm", "cli", "kill-pane",
                        "--pane-id", tostring(pane_id),
                    })
                    return
                end
            end
        end

        -- Dashboard doesn't exist, create it
        local direction = M.config.sidebar_position
        pane:split({
            direction = direction,
            size = M.config.sidebar_percent / 100.0,
            args = { bin },
        })
    end)
end

-- ---------------------------------------------------------------------------
-- Apply to config
-- ---------------------------------------------------------------------------

function M.apply_to_config(config)
    -- Register status bar handler
    if M.config.show_status_bar then
        wezterm.on("update-status", function(window, pane)
            local status = build_status_bar()
            if status ~= "" then
                window:set_right_status(status)
            end
        end)
    end

    -- Add toggle keybinding
    config.keys = config.keys or {}
    table.insert(config.keys, {
        key = M.config.toggle_key.key,
        mods = M.config.toggle_key.mods,
        action = create_toggle_action(),
    })

    return config
end

return M
