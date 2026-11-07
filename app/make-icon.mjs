// Generates a 1024x1024 app icon PNG (no image deps needed).
import { deflateSync } from "node:zlib";
import { writeFileSync } from "node:fs";

const S = 1024;
const buf = Buffer.alloc(S * S * 4);
const inRounded = (x, y, r) => {
  // squircle-ish: rounded square with margin
  const m = 96, rad = 220;
  const cx = Math.min(Math.max(x, m + rad), S - m - rad);
  const cy = Math.min(Math.max(y, m + rad), S - m - rad);
  const dx = x - cx, dy = y - cy;
  return dx * dx + dy * dy <= rad * rad;
};
for (let y = 0; y < S; y++) {
  for (let x = 0; x < S; x++) {
    const i = (y * S + x) * 4;
    const t = (x + y) / (2 * S);
    let r = 0, g = 0, b = 0, a = 0;
    if (inRounded(x, y)) {
      // vertical indigo gradient
      r = Math.round(96 + 60 * t);
      g = Math.round(112 + 60 * t);
      b = Math.round(255 - 20 * t);
      a = 255;
      // concentric "disc" ring
      const dx = x - S / 2, dy = y - S / 2;
      const d = Math.sqrt(dx * dx + dy * dy);
      if (d > 250 && d < 380) { r = 235; g = 240; b = 255; }
      if (d < 70) { r = 11; g = 13; b = 18; }
    }
    buf[i] = r; buf[i + 1] = g; buf[i + 2] = b; buf[i + 3] = a;
  }
}
// PNG encode
const raw = Buffer.alloc((S * 4 + 1) * S);
for (let y = 0; y < S; y++) {
  raw[y * (S * 4 + 1)] = 0;
  buf.copy(raw, y * (S * 4 + 1) + 1, y * S * 4, (y + 1) * S * 4);
}
const crcTable = [...Array(256)].map((_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
const crc = (b) => {
  let c = 0xffffffff;
  for (const byte of b) c = crcTable[(c ^ byte) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
};
const chunk = (type, data) => {
  const len = Buffer.alloc(4); len.writeUInt32BE(data.length);
  const td = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const c = Buffer.alloc(4); c.writeUInt32BE(crc(td));
  return Buffer.concat([len, td, c]);
};
const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(S, 0); ihdr.writeUInt32BE(S, 4);
ihdr[8] = 8; ihdr[9] = 6; ihdr[10] = 0; ihdr[11] = 0; ihdr[12] = 0;
const png = Buffer.concat([
  Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
  chunk("IHDR", ihdr),
  chunk("IDAT", deflateSync(raw, { level: 9 })),
  chunk("IEND", Buffer.alloc(0)),
]);
writeFileSync("app-icon.png", png);
console.log("wrote app-icon.png", png.length, "bytes");
