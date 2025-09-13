new_read_globals = {
    '_HOST',
    'config',
    'peripheral',
    'textutils',
    'http',
    'fs',
    os = {
        fields = {
            'getComputerID',
            'getComputerLabel',
            'setComputerLabel',
            'clock',
            'time',
            'day',
            'date',
            'run',
            'sleep',
            'shutdown',
            'reboot',
            'pullEvent',
        }
    }
}

ignore = {
    "212/self", -- allow unused self parameter in methods
    -- allow redefining success, err. common pattern with pcalls
    "4/success",
    "4/err",
    -- Allow unused variables if the name starts with a _
    "21/_.*",
    "23/_.*",
}
