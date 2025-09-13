local BOT_NAME = 'Axiom'

local HTTP_TIMEOUT_SECS = 60 -- 1m

local MessageSink = {}

-- noop by default
function MessageSink:init() end

--[[
    @param {string} sender
    @param {string} message
    @param {string} [target]
]]
function MessageSink:sendMessage(sender, message, target)
    error 'unimplemented'
end

local ChatBox = setmetatable(
    { chatBox = nil },
    { __index = MessageSink }
)

function ChatBox:init()
    local chatBox = peripheral.wrap 'top'
    if not chatBox then
        error("No chatBox peripheral found")
    end

    self.chatBox = chatBox
end

function ChatBox:sendMessage(sender, message, target)
    for _, paragraph in ipairs(message.paragraphs) do
        local formattedMessage, err = textutils.serializeJSON(paragraph, { unicode_strings = true })
        if not formattedMessage then
            error("Failed to serialize message: " .. err)
        end

        local success, err
        if target then
            success, err = self.chatBox.sendFormattedMessageToPlayer(formattedMessage, target, sender, '<>')
        else
            success, err = self.chatBox.sendFormattedMessage(formattedMessage, sender, '<>')
        end

        if not success then
            error("Failed to send message: " .. err)
        end

        os.sleep(0.25)
    end
end

local DiscordHook = setmetatable(
    { hook_url = nil, hook_url_file = 'discord_webhook.txt' },
    { __index = MessageSink }
)

function DiscordHook:init()
    local f = fs.open(self.hook_url_file, 'r')
    if not f then
        error("Failed to open " .. self.hook_url_file .. " for reading")
    end
    local url = f.readAll()
    f.close()

    assert(url and url ~= '', "Discord webhook URL is empty")

    self.hook_url = url
end

function DiscordHook:sendMessage(sender, message, target)
    if target ~= nil then
        -- We can't handle private messages in Discord
        return
    end

    local textPieces = {}
    for _, paragraph in ipairs(message.paragraphs) do
        for _, component in ipairs(paragraph) do
            table.insert(textPieces, component.text)
        end
    end

    local text = table.concat(textPieces)

    local resp, err, errResp = http.post(
        self.hook_url,
        textutils.serializeJSON(
            {
                content = text,
                username = BOT_NAME,
            },
            { unicode_strings = true }
        ),
        { ['Content-Type'] = 'application/json' }
    )

    if not resp then
        local errText = ""
        if errResp then
            errText = errResp.readAll()
        end

        error("HTTP request to Discord failed: " .. err .. " " .. errText)
    end

    resp.readAll()
    resp.close()
end

local MultiSink = setmetatable(
    { sinks = {} },
    { __index = MessageSink }
)

function MultiSink.new(...)
    return setmetatable({ sinks = { ... } }, { __index = MultiSink })
end

function MultiSink:init()
    for _, sink in ipairs(self.sinks) do
        sink:init()
    end
end

function MultiSink:sendMessage(sender, message, target)
    local errs = {}

    for _, sink in ipairs(self.sinks) do
        local success, err = pcall(sink.sendMessage, sink, sender, message, target)
        if not success then
            table.insert(errs, err)
        end
    end

    if #errs > 0 then
        error("Failed to send message to one of sinks: \n" .. table.concat(errs, '\n'))
    end
end

local Message = {
    TEXT_SCHEMA = {
        ['$schema'] = 'http://json-schema.org/draft-07/schema#',
        ['type'] = 'object',
        properties = {
            paragraphs = {
                ['type'] = 'array',
                items = {
                    ['type'] = 'array',
                    items = {
                        ['$ref'] = '#/definitions/text_object',
                    }
                }
            }
        },
        required = { 'paragraphs' },
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
                    shadow_color = { ['type'] = { 'number', 'null' } },
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

--[[
    @param {http.Response} resp
]]
function serverSideEvents(resp)
    assert(resp.getResponseHeaders()['Content-Type']:find 'text/event%-stream', "Response is not SSE")

    return function()
        local res = {}
        while true do
            local line = resp.readLine()
            if not line then break end

            local tag, payload = line:match '^(%w+):?%s*(.-)%s*$'
            if not tag then
                break
            end

            res[tag] = payload
        end

        return res
    end
end

local Model = {}

function Model:prompt()
    local f = fs.open('prompt.md', 'r')
    if not f then
        error("Failed to open prompt.md for reading")
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

function Model:getReply(user, msg, _role)
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
                    schema_definition = Message.TEXT_SCHEMA,
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
        timeout = HTTP_TIMEOUT_SECS
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

function Mistral:getReply(user, msg, _role)
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
        timeout = HTTP_TIMEOUT_SECS
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
        timeout = HTTP_TIMEOUT_SECS
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

function OpenAi:getReply(user, msg, role)
    local body = textutils.serializeJSON {
        conversation = self:getOrCreateConversation(),
        model = 'gpt-5',
        temperature = 1,
        tools = {
            { ['type'] = 'web_search' },
            { ['type'] = 'code_interpreter', container = { ['type'] = 'auto' } },
        },
        input = {
            {
                ['type'] = 'message',
                role = role or 'user',
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
                schema = Message.TEXT_SCHEMA,
                strict = true
            }
        },
        stream = true,
    }

    -- Send a message to OpenAI
    local resp, err, errResp = http.post {
        url = 'https://api.openai.com/v1/responses',
        body = body,
        headers = self:headers(body),
        timeout = HTTP_TIMEOUT_SECS
    }

    if not resp then
        local errText = ""
        if errResp then
            errText = errResp.readAll()
        end

        error("HTTP request to " .. self.name .. " failed: " .. err .. " " .. errText)
    end

    return self:readReplyStream(resp)
end

function OpenAi:readReplyStream(resp)
    local itemToListen

    for event in serverSideEvents(resp) do
        if event.event == 'error' then
            error("Error event from OpenAI: " .. (event.data or "no error details"))
        elseif event.event == 'response.output_item.added' then
            local object = textutils.unserializeJSON(event.data)
            if object.item.type == 'message' and object.item.role == 'assistant' then
                itemToListen = object.item.id
            end
        elseif event.event == 'response.output_text.done' then
            local object = textutils.unserializeJSON(event.data)
            if object.item_id == itemToListen then
                resp.readAll()
                resp.close()

                return textutils.unserializeJSON(object.text)
            end
        end
    end

    error 'Error: reached end of stream without receiving a complete response'
end

local logger = {
    errlog = fs.open('errors.log', 'a')
}

function logger:error(msg, opts)
    self.errlog.writeLine(msg)
    if msg:len() > 100 then
        msg = msg:sub(1, 100) .. '\nFull error written to errors.log'
    end

    if opts.suppress then
        io.stderr:write(msg .. '\n')
    else
        error(msg)
    end
end

local sink = MultiSink.new(DiscordHook, ChatBox)
sink:init()

local model = OpenAi

do
    print("Health check. Sending wake up message to " .. model.name)

    local success, ret = pcall(model.getReply, model, 'crazypitlord', 'Wake up', 'system')
    if not success then
        logger:error(ret)
    end

    local success, err = pcall(sink.sendMessage, sink, BOT_NAME, ret)
    if not success then
        logger:error(err)
    end

    print("Health check successful. Received reply from " .. model.name)
end

print("Listening to chat")
while true do
    local event, username, message, uuid, isHidden = os.pullEvent 'chat'

    if not string.find(message:lower(), BOT_NAME:lower(), nil, true) then
        goto continue
    end

    local target = isHidden and username or nil

    local reply
    local success, ret = pcall(model.getReply, model, username, message)
    if not success then
        logger:error(ret, { suppress = true })
        reply = { paragraphs = { { { text = "Error processing request. Check logs" } } } }
    else
        reply = ret
    end

    local success, err = pcall(sink.sendMessage, sink, BOT_NAME, reply, username)
    if not success then
        logger:error(err, { suppress = true })
    end

    ::continue::
end
