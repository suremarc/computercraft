local DiscordHook = require 'DiscordHook'

local success, hook = DiscordHook.createWebhook 'https://discord.com/api/webhooks/1413949188814667889/WvKZv5fWumxZQoIKWBMGZV2fEI1sfRYdlx90JTOVtKaG6nh1zSXo46BhOVqaApHphk6N'
if not success then
    error("Webhook connection failed (reason: " .. hook .. ")")
end

hook.send('this is a test message', nil, nil)
