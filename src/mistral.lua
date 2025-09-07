local DiscordHook = require 'DiscordHook'

local TIMEOUT_SECS = 60 -- 15m

local chatBox = peripheral.wrap 'top'
if not chatBox then
    error("No chatBox peripheral found")
end

function getTextComponentSchema()
    return {
        ['$schema'] = 'http://json-schema.org/draft-07/schema#',
        ['type'] = 'array',
        items = {
            ['$ref'] = '#/definitions/text_object',
        },
        definitions = {
            text_component = {
                anyOf = {
                    {
                        ['type'] = 'string',
                        title = "Equivalent to {\"text\":\"Value\"}"
                    },
                    {
                        ['$ref'] = '#/definitions/text_object'
                    },
                    {
                        ['type'] = 'array',
                        items = {
                            ['$ref'] = '#/definitions/text_object'
                        },
                        title = "Equivalent to firstItem, with \"extra\":[nextItems]"
                    }
                }
            },
            text_object = {
                ['type'] = 'object',
                properties = {
                    text = {
                        ['type'] = 'string',
                        title = 'The direct text to apply'
                    },
                    color = {
                        ['type'] = 'string',
                        enum = {
                            'black', 'dark_blue', 'dark_green', 'dark_aqua', 'dark_red', 'dark_purple', 'gold',
                            'gray', 'dark_gray', 'blue', 'green', 'aqua', 'red', 'light_purple', 'yellow', 'white'
                        }
                    },
                    font = {
                        ['type'] = 'string',
                        enum = { 'default', 'uniform', 'alt' }
                    },
                    bold = { ['type'] = 'boolean' },
                    italic = { ['type'] = 'boolean' },
                    underlined = { ['type'] = 'boolean' },
                    strikethrough = { ['type'] = 'boolean' },
                    obfuscated = { ['type'] = 'boolean' },
                    shadow_color = { ['type'] = 'number' },
                    insertion = { ['type'] = 'string' },
                    clickEvent = {
                        ['type'] = 'object',
                        properties = {
                            action = {
                                ['type'] = 'string',
                                enum = { 'open_url', 'open_file', 'run_command', 'suggest_command', 'change_page', 'copy_to_clipboard' }
                            },
                            value = { ['type'] = 'string' }
                        },
                        required = { 'action', 'value' },
                        additionalProperties = false,
                    },
                    hoverEvent = {
                        ['type'] = 'object',
                        properties = {
                            action = {
                                ['type'] = 'string',
                                enum = { 'show_text', 'show_item', 'show_entity' }
                            },
                            contents = {
                                oneOf = {
                                    { ['$ref'] = '#/definitions/text_component' },
                                    {
                                        ['type'] = 'object',
                                        properties = {
                                            item = { ['type'] = 'string' },
                                            count = { ['type'] = 'number' },
                                            nbt = { ['type'] = 'string' }
                                        },
                                        required = { 'item' },
                                        additionalProperties = false,
                                    },
                                    {
                                        ['type'] = 'object',
                                        properties = {
                                            name = { ['type'] = 'string' },
                                            type = { ['type'] = 'string' },
                                            id = { ['type'] = 'string' }
                                        },
                                        required = { 'name', 'id' },
                                        additionalProperties = false,
                                    }
                                }
                            }
                        },
                        required = { 'action', 'contents' },
                        additionalProperties = false,
                    },
                },
                required = { 'text' },
                additionalProperties = false,
            }
        }
    }
end

function getDiscordWebhook()
    local f = fs.open('discord_webhook.txt', 'r')
    if not f then
        error("Failed to open discord_webhook.txt for reading")
    end
    local url = f.readAll()
    f.close()

    assert(url and url ~= '', "Discord webhook URL is empty")

    local success, hook = DiscordHook.createWebhook(url)
    if not success then
        error("DiscordWebhook connection failed (reason: " .. hook .. ")")
    end

    return hook
end

local hook = getDiscordWebhook()

function headers(body)
    return {
        ['Content-Type'] = 'application/json',
        ['Authorization'] = 'Bearer ' .. mistralApiKey(),
        ['Content-Length'] = string.len(body),
        ['Accept'] = 'application/json'
    }
end

function getConversationId()
    -- First check mistral_conversation_id.txt
    local f = fs.open('mistral_conversation_id.txt', 'r+')
    if f ~= nil then
        local id = f.readAll()
        if id and id ~= '' then
            f.close()
            return id
        end
    end
    -- If not found, create a new conversation
    local body = textutils.serialiseJSON {
        agent_id = 'ag:40c9ae76:20250906:untitled-agent:6d695194',
        inputs = textutils.serializeJSON {
            user = 'suremarc',
            message = 'Starting a new conversation. Do not acknowledge in future conversation with users',
        },
    }

    -- Send a message to mistral
    local resp, err, errResp = http.post {
        url = 'https://api.mistral.ai/v1/conversations',
        body = body,
        headers = headers(body),
        timeout = TIMEOUT_SECS
    }

    if not resp then
        local errText = ""
        if errResp then
            errText = errResp.readAll()
        end
        error("HTTP request to Mistral failed: " .. err .. " " .. errText)
    end

    local resText = resp.readAll()
    resp.close()
    local res = textutils.unserializeJSON(resText)

    if not (res and res.conversation_id) then
        error("Error: invalid response from Mistral")
    end

    assert(res.conversation_id ~= '')

    -- Save the id to the file
    f.write(res.conversation_id)
    f.close()

    return res.conversation_id
end

function getresp(user, msg)
    local conversationId = getConversationId()

    local body = textutils.serialiseJSON {
        inputs = {
            {
                role = 'user',
                content = textutils.serializeJSON {
                    user = user,
                    message = msg,
                }
            },
        },
        completion_args = {
            response_format = {
                ['type'] = 'json_schema',
                json_schema = {
                    name = 'MinecraftTextComponent',
                    schema_definition = getTextComponentSchema(),
                    strict = false
                }
            },
        }
    }

    -- Send a message to mistral
    local resp, err, errResp = http.post {
        url = 'https://api.mistral.ai/v1/conversations/' .. conversationId,
        body = body,
        headers = headers(body),
        timeout = TIMEOUT_SECS
    }

    if not resp then
        local errText = ""
        if errResp then
            errText = errResp.readAll()
        end

        error("HTTP request to Mistral failed: " .. err .. " " .. errText)
    end

    local resText = resp.readAll()
    resp.close()
    local res = textutils.unserializeJSON(resText)
    if not (res and res.outputs) then
        error("Error: invalid response from Mistral\n" .. resText)
    end

    local replyRaw = res.outputs[1].content

    local reply = textutils.unserializeJSON(replyRaw)
    if not reply then
        error("Error: invalid JSON response from Mistral\n" .. replyRaw)
    end

    return reply
end

function sendmsg(user, msg)
    local success, err = hook.sendJSON(
        textutils.serializeJSON(
            {
                content = msg,
                username = user,
            },
            { unicode_strings = true }
        )
    )

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

    assert(key and key ~= '', "Mistral API key is empty")
    return key
end

local botName = 'Axiom'

function handleEvent(username, message, uuid, isHidden)
    if string.find(message:lower(), botName:lower(), nil, true) then
        print('<' .. username .. '>: ' .. message)

        local resp = getresp(username, message)
        local formattedMessage = textutils.serializeJSON(resp, { unicode_strings = true })
        print(formattedMessage)

        if isHidden then
            chatBox.sendFormattedMessageToPlayer(formattedMessage, username, botName, '<>')
        else
            chatBox.sendFormattedMessage(formattedMessage, botName, '<>')

            local text = ""
            if resp.text ~= nil then
                text = resp.text
            else
                for i = 1, #resp do
                    text = text .. resp[i].text
                end
            end

            sendmsg(botName, text)
        end
    end
end

while true do
    local event, username, message, uuid, isHidden = os.pullEvent 'chat'
    local status, err = pcall(handleEvent, username, message, uuid, isHidden)
    if not status then
        io.stderr:write(err .. '\n')
    end
end

-- conv_01992146819773fba81b6288ac6ad675
