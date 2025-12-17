local mod = {}

function mod.on_load()
    print("Hello from lua!!!!!!!!!!!!!!!")
end

function mod.math_ops_test(a, b)
    return a * b
end

function mod.string_test(msg)
    print("Lua received message: " .. msg)
    return "Now returning a message to Turing."
end

return mod