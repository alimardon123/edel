-- Edel OS (roadmap M6.14b): an app's microphone, as the person's
-- apps.microphone key says. No portal asks for the microphone, and a
-- Flatpak app with the sound socket reaches PipeWire itself, so this hook
-- gives every Flatpak client the permission manager WirePlumber's own
-- client/find-flatpak-access.lua would (read and run every object), with
-- one rule more: an app whose entry in the portals' permission store
-- (table "devices", id "microphone", the Device portal's old place, which
-- shell-ui writes from the key) is "no" cannot see any microphone, so it
-- can neither record nor be linked to one. A change in the store applies
-- at once, to apps already running too.

log = Log.open_topic ("s-client")
store = Plugin.find ("portal-permissionstore")
cached = nil
loaded = false

-- The store's entries for the microphone: app id and its values.
function entries ()
  if not loaded and store ~= nil then
    cached = store:call ("lookup", "devices", "microphone")
    loaded = true
  end
  return cached
end

-- Whether the store says "no" for app_id.
function refused (app_id)
  local all = entries ()
  if all == nil or app_id == nil or app_id == "" then
    return false
  end
  for id, values in pairs (all) do
    if id == app_id then
      for _, value in pairs (values) do
        if value == "no" then
          return true
        end
      end
    end
  end
  return false
end

edel_pm = PermissionManager ()
edel_pm:set_default_permissions (Perm.RX)

edel_pm:add_interest_match (
  function (_, client, _)
    local app_id = client:get_property ("pipewire.access.portal.app_id")
    if refused (app_id) then
      log:info (client, "microphones hidden from " .. app_id)
      return Perm.NONE
    end
    return Perm.RX
  end,
  Interest {
    type = "node",
    Constraint { "media.class", "matches", "Audio/Source*" },
  }
)

if store ~= nil then
  store:connect ("changed", function (_, table, id, deleted, permissions)
    if table == "devices" and id == "microphone" then
      cached = permissions
      loaded = true
      edel_pm:update_permissions ()
    end
  end)
end

SimpleEventHook {
  name = "client/find-edel-microphone-access",
  before = "client/find-flatpak-access",
  after = "client/find-config-access",
  interests = {
    EventInterest {
      Constraint { "event.type", "=", "select-access" },
    },
  },
  execute = function (event)
    local client = event:get_subject ()
    -- The config's own rules win, as WirePlumber's do.
    if event:get_data ("permission-manager") ~= nil then
      return
    end
    if client:get_property ("pipewire.access") ~= "flatpak" then
      return
    end
    -- A Flatpak app that manages sound (a mixer) keeps WirePlumber's way.
    if client:get_property ("media.category") == "Manager" then
      return
    end
    if event:get_data ("effective-access") == nil then
      event:set_data ("effective-access", "flatpak")
    end
    event:set_data ("permission-manager", edel_pm)
  end
}:register ()
