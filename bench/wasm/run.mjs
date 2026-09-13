// node run.mjs <file.wasm> [label]: throughput per crate and per JS route, medians of 7.
import { readFileSync } from "node:fs";
import { performance } from "node:perf_hooks";

const WASM = ["tape-base64", "base64-simd", "base64"];
const SIZES = [64, 256, 1024, 4096, 16384, 65536, 262144, 1048576];
const [file, label = file] = process.argv.slice(2);
const { instance } = await WebAssembly.instantiate(readFileSync(file), {});
const { memory, alloc, encoded_len, encode, decode } = instance.exports;
const view = () => new Uint8Array(memory.buffer);
const latin1 = new TextDecoder("latin1");
const ascii = new TextEncoder();
const hasToBase64 = typeof Uint8Array.prototype.toBase64 === "function";

function time(call) {
  let iters = 1;
  for (;;) {
    const start = performance.now();
    call(iters);
    const ms = performance.now() - start;
    if (ms >= 60) return ms / iters;
    iters = Math.max(iters * 2, Math.ceil((iters * 80) / Math.max(ms, 0.01)));
  }
}
const median = (s) => [...s].sort((a, b) => a - b)[s.length >> 1];
const gibs = (bytes, ms) => bytes / (ms / 1000) / 2 ** 30;
const measure = (bytes, call) => {
  const s = [];
  for (let rep = 0; rep < 7; rep++) s.push(time(call));
  return gibs(bytes, median(s));
};

// The classic browser route, chunked so String.fromCharCode does not overflow the stack.
function btoaBytes(u8) {
  let s = "";
  for (let i = 0; i < u8.length; i += 8192) s += String.fromCharCode.apply(null, u8.subarray(i, i + 8192));
  return btoa(s);
}
function atobBytes(str) {
  const s = atob(str);
  const out = new Uint8Array(s.length);
  for (let i = 0; i < s.length; i++) out[i] = s.charCodeAt(i);
  return out;
}

// tape-base64 as a JS caller sees it: bytes copied into wasm memory, a string out.
function wasmEncodeToString(u8, input, encoded, encLen) {
  view().set(u8, input);
  const n = encode(0, input, u8.length, encoded, encLen, 1);
  return latin1.decode(view().subarray(encoded, encoded + n));
}
function wasmDecodeFromString(str, encoded, encLen, out, size) {
  const { written } = ascii.encodeInto(str, view().subarray(encoded, encoded + encLen));
  const n = decode(0, encoded, written, out, size + 16, 1);
  return view().slice(out, out + n);
}

const JS = ["tape-base64 via JS", "Buffer", "btoa/atob", ...(hasToBase64 ? ["toBase64/fromBase64"] : [])];
const rows = { encode: [], decode: [] };
for (const size of SIZES) {
  const encLen = encoded_len(size);
  const input = alloc(size), encoded = alloc(encLen), out = alloc(size + 16);
  const u8 = new Uint8Array(size);
  for (let i = 0; i < size; i++) u8[i] = (Math.imul(i, 2654435761) >>> 24) ^ (i & 0xff);
  view().set(u8, input);

  const written = encode(0, input, size, encoded, encLen, 1);
  if (written !== encLen) throw new Error(`tape-base64 wrote ${written}, expected ${encLen}`);
  const reference = view().slice(encoded, encoded + encLen);
  const str = latin1.decode(reference);
  if (Buffer.from(u8).toString("base64") !== str) throw new Error(`Buffer encodes differently at ${size}`);
  for (const which of [1, 2]) {
    const scratch = alloc(encLen);
    encode(which, input, size, scratch, encLen, 1);
    if (Buffer.compare(Buffer.from(reference), Buffer.from(view().slice(scratch, scratch + encLen))) !== 0) throw new Error(`${WASM[which]} encodes differently at ${size}`);
  }
  for (const which of [0, 1, 2]) {
    const n = decode(which, encoded, encLen, out, size + 16, 1);
    if (n !== size) throw new Error(`${WASM[which]} decoded ${n} bytes, expected ${size}`);
  }
  if (Buffer.compare(Buffer.from(wasmDecodeFromString(str, encoded, encLen, out, size)), Buffer.from(u8)) !== 0) throw new Error(`round trip via JS differs at ${size}`);

  const enc = [], dec = [];
  for (const which of [0, 1, 2]) {
    enc.push(measure(size, (iters) => encode(which, input, size, encoded, encLen, iters)));
    dec.push(measure(size, (iters) => decode(which, encoded, encLen, out, size + 16, iters)));
  }
  const loop = (f) => (iters) => { for (let i = 0; i < iters; i++) f(); };
  enc.push(measure(size, loop(() => wasmEncodeToString(u8, input, encoded, encLen))));
  dec.push(measure(size, loop(() => wasmDecodeFromString(str, encoded, encLen, out, size))));
  enc.push(measure(size, loop(() => Buffer.from(u8.buffer, u8.byteOffset, size).toString("base64"))));
  dec.push(measure(size, loop(() => Buffer.from(str, "base64"))));
  enc.push(measure(size, loop(() => btoaBytes(u8))));
  dec.push(measure(size, loop(() => atobBytes(str))));
  if (hasToBase64) {
    enc.push(measure(size, loop(() => u8.toBase64())));
    dec.push(measure(size, loop(() => Uint8Array.fromBase64(str))));
  }
  rows.encode.push([size, ...enc]);
  rows.decode.push([size, ...dec]);
}

const cols = [...WASM, ...JS];
console.log(`## ${label} (node ${process.version}, ${hasToBase64 ? "with" : "without"} Uint8Array.toBase64)\n`);
for (const direction of ["encode", "decode"]) {
  console.log(`### ${direction} (GiB/s of raw bytes; the first three are inside wasm, the rest are what JS sees)\n`);
  console.log(`| bytes | ${cols.join(" | ")} |`);
  console.log(`|---|${"---|".repeat(cols.length)}`);
  for (const [size, ...vals] of rows[direction]) console.log(`| ${size} | ${vals.map((v) => v.toFixed(2)).join(" | ")} |`);
  console.log();
}
