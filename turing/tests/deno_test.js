// Minimal Deno test script used by Rust integration test
// Expose a simple function that returns a number and an object id
function add(a, b) {
  return a + b;
}

function makeOpaque(id) {
  // return numeric id representing opaque pointer
  return id;
}

globalThis.add = add;
globalThis.makeOpaque = makeOpaque;
