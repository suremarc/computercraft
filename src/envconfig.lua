local envConfig = {}

for line in io.lines '.env' do
    local key, value = line:match '^%s*([%w_]+)%s*=%s*(.-)%s*$'
    if key and value then
        envConfig[key] = value
    end
end

envConfig.BOT_NAME = envConfig.BOT_NAME or 'Axiom'
envConfig.DISCORD_TEST_THREAD_ID = envConfig.DISCORD_TEST_THREAD_ID or '1416490212892217378'
envConfig.HTTP_TIMEOUT_SECS = tonumber(envConfig.HTTP_TIMEOUT_SECS) or 60

return envConfig
