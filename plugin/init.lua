-- wezterm-agent-dashboard / plugin/init.lua
-- WezTerm Lua module for agent dashboard integration.
--
-- Provides:
--   1. Status bar summary showing agent counts by status
--   2. Keybinding to toggle the dashboard sidebar
--   3. Optional tab status styling for inactive agent tabs
--
-- Usage in wezterm.lua:
--   local dashboard = wezterm.plugin.require("https://github.com/0maru/wezterm-agent-dashboard")
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
    tab_status = {
        enabled = false,
        reset_on_active = true,
        style_active_tab = false,
        states = {
            notification = { icon = "🔔", bg_color = "#3b2f00", fg_color = "#ffd75f" },
            error = { icon = "✕", bg_color = "#3a1f1f", fg_color = "#ff5f5f" },
            waiting = { icon = "◐", bg_color = "#332b12", fg_color = "#ffd75f" },
            running = { icon = "●", bg_color = "#16351f", fg_color = "#87d787" },
        },
    },
}

local function merge_config(dst, src)
    for k, v in pairs(src) do
        if type(v) == "table" and type(dst[k]) == "table" then
            merge_config(dst[k], v)
        else
            dst[k] = v
        end
    end
end

function M.setup(user_config)
    if user_config then
        merge_config(M.config, user_config)
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
-- Tab status: inactive tab icon/color
-- ---------------------------------------------------------------------------

local dismissed_attention_by_pane = {}

local tab_state_priority = {
    notification = 100,
    error = 90,
    waiting = 80,
    running = 70,
    idle = 10,
}

local function tab_title(tab)
    local title = tab.tab_title
    if title and #title > 0 then
        return title
    end

    if tab.active_pane and tab.active_pane.title then
        return tab.active_pane.title
    end

    return "tab"
end

local function pane_key(pane)
    if not pane or not pane.pane_id then
        return nil
    end
    return tostring(pane.pane_id)
end

local function pane_attention_key(vars)
    if not vars or not vars.agent_attention or vars.agent_attention == "" then
        return nil
    end
    return "attention:" .. vars.agent_attention
end

local function style_for_state(state_name)
    local states = M.config.tab_status.states or {}
    return states[state_name] or states.notification
end

local function state_for_pane(pane)
    local vars = pane and pane.user_vars
    if not vars or not vars.agent_type then
        return nil
    end

    local attention_key = pane_attention_key(vars)
    local key = pane_key(pane)
    if attention_key and key and dismissed_attention_by_pane[key] ~= attention_key then
        local attention_name = vars.agent_attention
        return {
            name = attention_name,
            style = style_for_state(attention_name),
            priority = tab_state_priority[attention_name] or tab_state_priority.notification,
        }
    end

    local status = vars.agent_status
    if status and status ~= "" then
        local style = (M.config.tab_status.states or {})[status]
        if style then
            return {
                name = status,
                style = style,
                priority = tab_state_priority[status] or 0,
            }
        end
    end

    return nil
end

local function state_for_tab(tab)
    local best = nil

    for _, pane in ipairs(tab.panes or {}) do
        local state = state_for_pane(pane)
        if state and state.style and (not best or state.priority > best.priority) then
            best = state
        end
    end

    return best
end

local function mark_tab_attention_seen(tab)
    if not M.config.tab_status.reset_on_active then
        return
    end

    for _, pane in ipairs(tab.panes or {}) do
        local vars = pane.user_vars
        local attention_key = pane_attention_key(vars)
        local key = pane_key(pane)
        if attention_key and key then
            dismissed_attention_by_pane[key] = attention_key
        end
    end
end

local function build_tab_status_title(tab, state, max_width)
    local style = state.style or {}
    local icon = style.icon
    local title = tab_title(tab)
    local prefix = ""

    if icon and icon ~= "" then
        prefix = icon .. " "
    end

    local text = prefix .. title
    if max_width and max_width > 2 then
        text = wezterm.truncate_right(text, max_width - 2)
    end

    local items = {}
    if style.bg_color then
        table.insert(items, { Background = { Color = style.bg_color } })
    end
    if style.fg_color then
        table.insert(items, { Foreground = { Color = style.fg_color } })
    end
    table.insert(items, { Text = " " .. text .. " " })

    return items
end

local function register_tab_status_handlers()
    wezterm.on("user-var-changed", function(window, pane, name, value)
        if name == "agent_attention" then
            local ok, pane_id = pcall(function()
                return pane:pane_id()
            end)
            if ok and pane_id then
                dismissed_attention_by_pane[tostring(pane_id)] = nil
            end
        end
    end)

    wezterm.on("format-tab-title", function(tab, tabs, panes, config, hover, max_width)
        if tab.is_active then
            mark_tab_attention_seen(tab)
            if not M.config.tab_status.style_active_tab then
                return tab_title(tab)
            end
        end

        local state = state_for_tab(tab)
        if not state then
            return tab_title(tab)
        end

        return build_tab_status_title(tab, state, max_width)
    end)
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
    if M.config.tab_status.enabled then
        register_tab_status_handlers()
    end

    -- Register status bar handler
    if M.config.show_status_bar then
        wezterm.on("update-status", function(window, pane)
            local status = build_status_bar()
            window:set_right_status(status)
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
