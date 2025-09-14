local function isTestEnvironment()
    return _HOST:find 'CraftOS' ~= nil
end

if isTestEnvironment() then
    config.set('abortTimeout', 120000)
end

local envConfig = require 'envconfig'

local MessageSink = {}

-- noop by default
function MessageSink:init() end

--[[
    @param {string} sender
    @param {string} message
    @param {string} [target]
]]
function MessageSink:sendMessage(_sender, _message, _target)
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
    { hook_url = nil },
    { __index = MessageSink }
)

function DiscordHook:init()
    local url = envConfig.DISCORD_WEBHOOK_URL
    assert(url and url ~= '', "Discord webhook URL is empty")
    self.hook_url = url
end

function DiscordHook:sendMessage(_sender, message, target)
    if target ~= nil then
        -- We can't handle private messages in Discord
        return
    end

    local textPieces = {}
    for _, paragraph in ipairs(message.paragraphs) do
        for _, component in ipairs(paragraph) do
            table.insert(textPieces, component.text)
        end

        table.insert(textPieces, '\n')
    end

    local text = table.concat(textPieces)

    local mainBody = {
        content = text,
        username = envConfig.BOT_NAME,
    }

    local formData = {
        '\r\n--boundary',
        '\nContent-Disposition: form-data; name="payload_json"',
        '\nContent-Type: application/json',
        '\n',
        '\nJSON PLACEHOLDER',
    }

    for i, image in ipairs(message.images or {}) do
        if mainBody.attachments == nil then
            mainBody.attachments = {}
        end

        table.insert(mainBody.attachments, {
            id = i - 1,
            filename = image.filename,
        })

        table.insert(formData, '\r\n--boundary')
        table.insert(formData,
            '\nContent-Disposition: form-data; name="files[' .. (i - 1) .. ']"; filename="' .. image.filename .. '"')
        table.insert(formData, '\nContent-Type: image/png')
        table.insert(formData, '\nContent-Transfer-Encoding: base64')
        table.insert(formData, '\n')
        table.insert(formData, '\n' .. image.data)
    end

    table.insert(formData, '\r\n--boundary--')

    formData[5] = '\n' .. textutils.serializeJSON(mainBody, { unicode_strings = true })

    local resp, err, errResp = http.post(
        self.hook_url .. (isTestEnvironment() and '?thread_id=' .. envConfig.DISCORD_TEST_THREAD_ID or ''),
        table.concat(formData),
        { ['Content-Type'] = 'multipart/form-data; boundary=boundary' }
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
                minItems = 1,
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
                        title = 'The direct text to apply',
                        pattern = '.+'
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
                                enum = {
                                    'open_url', 'open_file', 'run_command',
                                    'suggest_command', 'change_page', 'copy_to_clipboard'
                                }
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
                required = {
                    'text', 'color', 'font', 'bold', 'italic', 'underlined', 'strikethrough',
                    'obfuscated', 'shadow_color', 'insertion', 'clickEvent', 'hoverEvent'
                },
                additionalProperties = false,
            }
        }
    }
}

--[[
    @param {http.Response} resp
]]
local function serverSentEvents(resp)
    assert(resp.getResponseHeaders()['Content-Type']:find 'text/event%-stream', "Response is not SSE")

    local done = false
    return function()
        if done then return end

        local res = {}
        while true do
            local line = resp.readLine()
            if not line then
                done = true
                break
            end

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
    error 'unimplemented'
end

function Model:getOrCreateConversation()
    -- First check if it's saved
    local id
    if fs.exists(self.conversationIdFile) then
        local f, err = fs.open(self.conversationIdFile, 'r')
        if err then
            error("Error opening " .. self.conversationIdFile .. ": " .. err)
        end

        id = f.readAll()
        f.close()
    end

    if not id or id == '' then
        id = self:createConversation()

        local f, err = fs.open(self.conversationIdFile, 'w')
        if err then
            error("Error opening " .. self.conversationIdFile .. " for writing: " .. err)
        end

        f.write(id)
        f.close()
    end

    return id
end

function Model:getReply(_user, _msg, _role)
    error 'unimplemented'
end

local Mistral = setmetatable(
    {
        name = 'Mistral',
        conversationIdFile = 'mistral_conversation_id.txt',
    },
    { __index = Model }
)

function Mistral:apiKey()
    local key = envConfig.MISTRAL_API_KEY
    assert(key and key ~= '', "Mistral API key is empty")
    return key
end

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
        timeout = envConfig.HTTP_TIMEOUT_SECS
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
        timeout = envConfig.HTTP_TIMEOUT_SECS
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

local OpenAI = setmetatable(
    {
        name = 'OpenAI',
        conversationIdFile = 'openai_conversation_id.txt',
    },
    { __index = Model }
)

function OpenAI:apiKey()
    local key = envConfig.OPENAI_API_KEY
    assert(key and key ~= '', "OpenAI API key is empty")
    return key
end

function OpenAI:createConversation()
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
        timeout = envConfig.HTTP_TIMEOUT_SECS
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

function OpenAI:getReply(user, msg, role)
    local body = textutils.serializeJSON {
        conversation = self:getOrCreateConversation(),
        model = isTestEnvironment() and 'gpt-5-nano' or 'gpt-5',
        temperature = 1,
        tools = {
            { ['type'] = 'web_search' },
            { ['type'] = 'code_interpreter', container = { ['type'] = 'auto' } },
            { ['type'] = 'image_generation' },
            -- WIP
            -- {
            --     ['type'] = 'mcp',
            --     server_label = 'github',
            --     server_description = 'ComputerCraft GitHub code, including the code for this bot',
            --     server_url = 'https://api.githubcopilot.com/mcp/'
            -- }
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
        timeout = envConfig.HTTP_TIMEOUT_SECS
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

function OpenAI:readReplyStream(resp)
    local outputs = {}

    local completed = {
        paragraphs = {},
        images = {}
    }

    for event in serverSentEvents(resp) do
        local object, err
        if event.data then
            object, err = textutils.unserializeJSON(event.data)
        end

        if err then
            error("Failed to parse event data: " .. err .. "\nData: " .. (event.data or "nil"))
        end

        if event.event == 'error' then
            error("Error event from OpenAI: " .. (event.data or "no error details"))
        elseif event.event == 'response.output_item.added' then
            outputs[object.output_index + 1] = object
        elseif event.event == 'response.output_text.done' then
            local original = outputs[object.output_index + 1]
            if original.item.type == 'message' and original.item.role == 'assistant' then
                local payload, err = textutils.unserializeJSON(object.text)
                if err then
                    error("Failed to parse output_text: " .. err .. "\nText: " .. (object.text or "nil"))
                end

                completed.paragraphs = payload.paragraphs
            end
        elseif event.event == 'response.image_generation_call.partial_image' then
            local output = outputs[object.output_index + 1]
            if output.parts == nil then
                output.parts = {}
            end

            output.parts[object.partial_image_index + 1] = object.partial_image_b64
        elseif event.event == 'response.image_generation_call.done' then
            local output = outputs[object.output_index + 1]
            output.result = table.concat(output.parts or {})
            completed.images[object.output_index + 1] = {
                filename = object.item_id .. '.png',
                data = output.result
            }
        end
    end

    resp.readAll()
    resp.close()

    return completed
end

--[[  Main program  ]]

local logger = {
    errlog = fs.open('errors.log', 'a')
}

function logger:error(msg, opts)
    self.errlog.writeLine(msg)
    self.errlog.flush()
    if msg:len() > 100 then
        msg = msg:sub(1, 100) .. '\nFull error written to errors.log'
    end

    if opts and opts.fatal then
        error(msg)
    else
        io.stderr:write(msg .. '\n')
    end
end

-- ChatBox is not available in CraftOS-PC
local sink = isTestEnvironment() and MultiSink.new(DiscordHook) or MultiSink.new(DiscordHook, ChatBox)
sink:init()

local model = OpenAI

do
    print("Health check. Sending wake up message to " .. model.name)

    local success, ret = pcall(model.getReply, model, 'crazypitlord', 'Wake up', 'system')
    if not success then
        logger:error(ret, { fatal = true })
    end

    local success, err = pcall(sink.sendMessage, sink, envConfig.BOT_NAME, ret)
    if not success then
        logger:error(err, { fatal = true })
    end

    print("Health check successful. Received reply from " .. model.name)
end

if isTestEnvironment() then
    os.shutdown()
end

print("Listening to chat")
while true do
    local _event, username, message, _uuid, isHidden = os.pullEvent 'chat'

    if not string.find(message:lower(), envConfig.BOT_NAME:lower(), nil, true) then
        goto continue
    end

    print('<' .. username .. '>: ' .. message)

    local reply
    local success, ret = pcall(model.getReply, model, username, message)
    if not success then
        logger:error(ret)
        reply = { paragraphs = { { { text = "Error processing request. Check logs" } } } }
    else
        reply = ret
    end

    local success, err = pcall(sink.sendMessage, sink, envConfig.BOT_NAME, reply, isHidden and username or nil)
    if not success then
        logger:error(err)
    end

    ::continue::
end
