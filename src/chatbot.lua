local DiscordHook = require 'DiscordHook'

local TIMEOUT_SECS = 60 -- 1m

local chatBox = peripheral.wrap 'top'
if not chatBox then
    error("No chatBox peripheral found")
end

local hook

local BOT_NAME = 'Axiom'

function handleEvent(model, username, message, uuid, isHidden)
    if string.find(message:lower(), BOT_NAME:lower(), nil, true) then
        print('<' .. username .. '>: ' .. message)

        local resp = model:getReply(username, message)
        local formattedMessage = textutils.serializeJSON(resp.components, { unicode_strings = true })
        print(formattedMessage)

        if isHidden then
            chatBox.sendFormattedMessageToPlayer(formattedMessage, username, BOT_NAME, '<>')
        else
            chatBox.sendFormattedMessage(formattedMessage, BOT_NAME, '<>')

            local text = ""
            for i = 1, #resp.components do
                text = text .. resp.components[i].text
            end

            sendMsgToDiscord(BOT_NAME, text)
        end
    end
end

local Model = {
    OUTPUT_SCHEMA = {
        ['$schema'] = 'http://json-schema.org/draft-07/schema#',
        ['type'] = 'object',
        properties = {
            components = {
                ['type'] = 'array',
                items = {
                    ['$ref'] = '#/definitions/text_object',
                }
            }
        },
        required = { 'components' },
        additionalProperties = false,
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
                        minItems = 1,
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
                        ['type'] = { 'string', 'null' },
                        enum = {
                            'black', 'dark_blue', 'dark_green', 'dark_aqua', 'dark_red', 'dark_purple', 'gold',
                            'gray', 'dark_gray', 'blue', 'green', 'aqua', 'red', 'light_purple', 'yellow', 'white'
                        }
                    },
                    font = {
                        ['type'] = { 'string', 'null' },
                        enum = { 'default', 'uniform', 'alt' }
                    },
                    bold = { ['type'] = { 'boolean', 'null' } },
                    italic = { ['type'] = { 'boolean', 'null' } },
                    underlined = { ['type'] = { 'boolean', 'null' } },
                    strikethrough = { ['type'] = { 'boolean', 'null' } },
                    obfuscated = { ['type'] = { 'boolean', 'null' } },
                    shadow_color = { ['type'] = { 'boolean', 'null' } },
                    insertion = { ['type'] = { 'string', 'null' } },
                    clickEvent = {
                        ['type'] = { 'object', 'null' },
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
                        ['type'] = { 'object', 'null' },
                        properties = {
                            action = {
                                ['type'] = 'string',
                                enum = { 'show_text', 'show_item', 'show_entity' }
                            },
                            contents = {
                                ['type'] = 'array',
                                minItems = 1,
                                items = {
                                    ['$ref'] = '#/definitions/text_object'
                                },
                            }
                        },
                        required = { 'action', 'contents' },
                        additionalProperties = false,
                    },
                },
                required = { 'text', 'color', 'font', 'bold', 'italic', 'underlined', 'strikethrough', 'obfuscated', 'shadow_color', 'insertion', 'clickEvent', 'hoverEvent' },
                additionalProperties = false,
            }
        }
    }
}

function Model:prompt()
    local f = fs.open('prompt.txt', 'r')
    if not f then
        error("Failed to open prompt.txt for reading")
    end
    local prompt = f.readAll()
    f.close()

    assert(prompt and prompt ~= '', "Prompt is empty")
    return prompt
end

function Model:headers(body)
    return {
        ['Content-Type'] = 'application/json',
        ['Authorization'] = 'Bearer ' .. self:apiKey(),
        ['Content-Length'] = string.len(body),
        ['Accept'] = 'application/json'
    }
end

function Model:apiKey()
    local f = fs.open(self.apiKeyFile, 'r')
    if not f then
        error("Failed to open " .. self.apiKeyFile .. " for reading")
    end
    local key = f.readAll()
    f.close()

    assert(key and key ~= '', self.name .. " API key is empty")
    return key
end

function Model:getOrCreateConversation()
    -- First check if it's saved
    local f, err = fs.open(self.conversationIdFile, 'r+')
    if err then
        error("Error opening " .. self.conversationIdFile .. ": " .. err)
    end

    local id = f.readAll()
    if not id or id == '' then
        id = self:createConversation()
        f.write(id)
    end

    f.close()
    return id
end

function Model:getReply(user, msg)
    error("unimplemented")
end

local Mistral = setmetatable(
    {
        name = 'Mistral',
        apiKeyFile = 'mistral_api_key.txt',
        conversationIdFile = 'mistral_conversation_id.txt',
    },
    { __index = Model }
)

function Mistral:createConversation()
    local body = textutils.serializeJSON {
        -- agent_id = 'ag:40c9ae76:20250906:untitled-agent:6d695194',
        model = 'mistral-large-2411',
        inputs = textutils.serializeJSON {
            user = 'suremarc',
            message = self:prompt(),
        },
        completion_args = {
            response_format = {
                ['type'] = 'json_object',
                json_schema = {
                    name = 'MinecraftTextComponent',
                    schema_definition = self.OUTPUT_SCHEMA,
                    strict = true
                }
            },
        }
    }

    -- Send a message to mistral
    local resp, err, errResp = http.post {
        url = 'https://api.mistral.ai/v1/conversations',
        body = body,
        headers = self:headers(body),
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

    return res.conversation_id
end

function Mistral:getReply(user, msg)
    local body = textutils.serializeJSON {
        inputs = {
            {
                role = 'user',
                content = textutils.serializeJSON {
                    user = user,
                    message = msg,
                }
            },
        },
    }

    -- Send a message to mistral
    local resp, err, errResp = http.post {
        url = 'https://api.mistral.ai/v1/conversations/' .. self:getOrCreateConversation(),
        body = body,
        headers = self:headers(body),
        timeout = TIMEOUT_SECS
    }

    if not resp then
        local errText = ""
        if errResp then
            errText = errResp.readAll()
        end

        error("HTTP request to " .. self.name .. " failed: " .. err .. " " .. errText)
    end

    local resText = resp.readAll()
    resp.close()
    local res = textutils.unserializeJSON(resText)
    if not (res and res.outputs) then
        error("Error: invalid response from " .. self.name .. "\n" .. resText)
    end

    local replyRaw = res.outputs[1].content

    local reply = textutils.unserializeJSON(replyRaw)
    if not reply then
        error("Error: invalid JSON response from " .. self.name .. "\n" .. replyRaw)
    end

    return reply
end

local OpenAi = setmetatable(
    {
        name = 'OpenAI',
        apiKeyFile = 'openai_api_key.txt',
        conversationIdFile = 'openai_conversation_id.txt',
    },
    { __index = Model }
)

function OpenAi:createConversation()
    local body = textutils.serializeJSON {
        items = {
            {
                ['type'] = 'message',
                role = 'system',
                content = self:prompt(),
            },
        },
    }

    -- Send a message to OpenAI
    local resp, err, errResp = http.post {
        url = 'https://api.openai.com/v1/conversations',
        body = body,
        headers = self:headers(body),
        timeout = TIMEOUT_SECS
    }

    if not resp then
        local errText = ""
        if errResp then
            errText = errResp.readAll()
        end
        error("HTTP request to OpenAI failed: " .. err .. " " .. errText)
    end

    local resText = resp.readAll()
    resp.close()
    local res = textutils.unserializeJSON(resText)

    if not (res and res.id) then
        error("Error: invalid response from OpenAI")
    end

    assert(res.id ~= '')

    return res.id
end

function OpenAi:getReply(user, msg)
    local body = textutils.serializeJSON {
        conversation = self:getOrCreateConversation(),
        model = 'gpt-4.1',
        temperature = 1,
        input = {
            {
                ['type'] = 'message',
                role = 'user',
                content = textutils.serializeJSON {
                    user = user,
                    message = msg,
                }
            },
        },
        text = {
            format = {
                ['type'] = 'json_schema',
                name = 'MinecraftTextComponent',
                schema = self.OUTPUT_SCHEMA,
                strict = true
            }
        },
    }

    -- Send a message to OpenAI
    local resp, err, errResp = http.post {
        url = 'https://api.openai.com/v1/responses',
        body = body,
        headers = self:headers(body),
        timeout = TIMEOUT_SECS
    }

    if not resp then
        local errText = ""
        if errResp then
            errText = errResp.readAll()
        end

        error("HTTP request to " .. self.name .. " failed: " .. err .. " " .. errText)
    end

    local resText = resp.readAll()
    resp.close()
    local res = textutils.unserializeJSON(resText)
    if not (res and res.output) then
        error("Error: invalid response from " .. self.name .. "\n" .. resText)
    end

    local replyRaw
    for i = 1, #res.output do
        if res.output[i].type == 'message' and res.output[i].role == 'assistant' and res.output[i].status == 'completed' then
            replyRaw = res.output[i].content[1].text
            break
        end
    end

    if not replyRaw then
        error("Error: no completed assistant message in response from " .. self.name .. "\n" .. resText)
    end

    local reply = textutils.unserializeJSON(replyRaw)
    if not reply then
        error("Error: invalid JSON response from " .. self.name .. "\n" .. replyRaw)
    end

    return reply
end

function sendMsgToDiscord(user, msg)
    print("Sending message to Discord: " .. msg)
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

do
    local f = fs.open('discord_webhook.txt', 'r')
    if not f then
        error("Failed to open discord_webhook.txt for reading")
    end
    local url = f.readAll()
    f.close()

    assert(url and url ~= '', "Discord webhook URL is empty")

    local success, newHook = DiscordHook.createWebhook(url)
    if not success then
        error("DiscordWebhook connection failed (reason: " .. newHook .. ")")
    end

    hook = newHook
    print("Webhook initialized")
end

local model = OpenAi

print("Listening to chat")
while true do
    local event, username, message, uuid, isHidden = os.pullEvent 'chat'
    local status, err = pcall(handleEvent, model, username, message, uuid, isHidden)
    if not status then
        io.stderr:write(err .. '\n')
    end
end
