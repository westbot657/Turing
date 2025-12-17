local mod = {}


function mod.on_load()
    turing_api.Log.info("Hello from lua!!!!!!!!!!!!!!!")
end

function mod.math_ops_test(a, b)
    return a * b
end

function mod.string_test(msg)
    turing_api.Log.info("Lua received message: " .. msg)
    return "Now returning a message to Turing."
end

return mod