local DiscordHook = require 'DiscordHook'

local success, hook = DiscordHook.createWebhook 'https://discord.com/api/webhooks/1413949188814667889/WvKZv5fWumxZQoIKWBMGZV2fEI1sfRYdlx90JTOVtKaG6nh1zSXo46BhOVqaApHphk6N'
if not success then
    error("DiscordWebhook connection failed (reason: " .. hook .. ")")
end

local msg = io.stdin.readLine()
sendmsg('suremarc', msg)

function sendmsg(user, msg)
    local headers = {
        ['Content-Type'] = 'application/json',
        ['Authorization'] = 'Bearer ' .. mistralApiKey()
    }

    -- Send a message to mistral
    local resp = http.post('https://api.mistral.ai/v1/agents/completions', textutils.serialiseJSON {
        agent_id = 'ag:40c9ae76:20250906:untitled-agent:6d695194',
        messages = {
            role = 'user',
            content = textutils.serializeJSON {
                user = user,
                message = msg,
            }
        }
    })

    if not resp then
        error("HTTP request to Mistral failed")
    end

    local resText = resp.readAll()
    resp.close()
    local res = textutils.unserializeJSON(resText)
    if not (res and res.choices) then
        error("Error: invalid response from Mistral")
    end

    local reply = result.choices[1].message.content

    local success, err = hook.send(reply, nil, nil)
    if not success then
        error("Failed to send message to Discord (reason: " .. err .. ")")
    end
end

function mistralApiKey()
    local f = fs.open('mistral_api_key.txt', 'r')
    if not f then
        error("Failed to open mistral_api_key.txt for reading")
    end
    local key = f.readAll()
    f.close()
    return key
end
