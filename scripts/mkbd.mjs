#!/usr/bin/env node
// Build a minimal but valid Blu-ray BDMV structure for testing BluForge
// without hardware. Generates a tiny H.264+AC3 clip via ffmpeg and writes
// index.bdmv / MovieObject.bdmv / PLAYLIST / CLIPINF / STREAM.
//
//   node scripts/mkbd.mjs <outdir> [seconds]
//
import { mkdirSync, writeFileSync, statSync, rmSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { join } from "node:path";

const outdir = process.argv[2];
const seconds = Number(process.argv[3] ?? 10);
if (!outdir) {
  console.error("usage: node scripts/mkbd.mjs <outdir> [seconds]");
  process.exit(1);
}

const VID_PID = 0x1011;
const AUD_PID = 0x1100;
const DURATION_45K = Math.round(seconds * 45000);
let IN_TIME = 0;
let OUT_TIME = DURATION_45K;

class BW {
  constructor() { this.bits = []; }
  push(val, n) { for (let i = n - 1; i >= 0; i--) this.bits.push((val >> i) & 1); }
  pushBytes(buf) { for (const byte of buf) this.push(byte, 8); }
  align() { while (this.bits.length % 8) this.bits.push(0); }
  bytes() {
    this.align();
    const b = Buffer.alloc(this.bits.length / 8);
    for (let i = 0; i < b.length; i++) {
      let v = 0;
      for (let j = 0; j < 8; j++) v = (v << 1) | this.bits[i * 8 + j];
      b[i] = v;
    }
    return b;
  }
}
const u8 = (n) => Buffer.from([n & 0xff]);
const u16 = (n) => { const b = Buffer.alloc(2); b.writeUInt16BE(n & 0xffff); return b; };
const u32 = (n) => { const b = Buffer.alloc(4); b.writeUInt32BE(n >>> 0); return b; };
const cat = (...bs) => Buffer.concat(bs);
const zeros = (n) => Buffer.alloc(n);
const ascii = (s) => Buffer.from(s, "ascii");

// ---------------------------------------------------------------- index.bdmv
function buildIndex() {
  // AppInfoBDMV block: 2 bytes of flags/format + 32 user bytes (len 34)
  const appBits = new BW();
  appBits.push(0, 1); appBits.push(0, 1); appBits.push(0, 1); appBits.push(0, 1);
  appBits.push(0, 4); appBits.push(6, 4); appBits.push(1, 4); // idrt=0, video_format=1080p(6), fps=23.976
  const appInfo = cat(appBits.bytes(), zeros(32));

  const idx = new BW();
  idx.push(1, 2);  // object_type = hdmv
  idx.push(0, 2);  // access_type = permitted
  idx.push(0, 28);
  idx.push(0, 2);  // playback_type = movie
  idx.push(0, 14);
  idx.push(0, 16); // id_ref (movie object 0)
  idx.push(0, 32);
  const title = idx.bytes();

  const firstPlay = cat(Buffer.from([0x40, 0, 0, 0]), zeros(8));
  const topMenu = cat(Buffer.from([0x40, 0, 0, 0]), cat(u16(0), u16(0xffff), zeros(4)));

  const indexBody = cat(firstPlay, topMenu, u16(1), title);
  const INDEX_START = 80;

  const head = cat(
    ascii("INDX"), ascii("0200"),
    u32(INDEX_START), u32(0),
    zeros(24),
    u32(34), appInfo,          // AppInfoBDMV at offset 40
    zeros(INDEX_START - 40 - 4 - 34),
  );
  const table = cat(u32(indexBody.length), indexBody);
  return cat(head, table);
}

// ----------------------------------------------------- MovieObject.bdmv (trivial)
function buildMovieObject() {
  return cat(
    ascii("MOBJ"), ascii("0200"), u32(0),
    zeros(40 - 12),
    u32(8),        // data_len
    u32(0),        // reserved
    u16(1),        // 1 movie object
    u8(0), u8(0),  // flags
    u16(0),        // 0 commands
  );
}

// ------------------------------------------------------------------ mpls
function buildMpls() {
  const stn = (() => {
    const b = new BW();
    b.push(0, 16);                     // reserved
    b.push(1, 8); b.push(1, 8);        // 1 video, 1 audio
    for (let i = 0; i < 6; i++) b.push(0, 8);
    b.push(0, 32);                     // reserved
    // video stream: block1 [type,pid], block2 [coding_type, format/rate]
    b.push(3, 8); b.push(1, 8); b.push(VID_PID, 16);
    b.push(2, 8); b.push(0x1b, 8); b.push(6, 4); b.push(1, 4);
    // audio stream: block1, block2 [coding_type, format/rate, lang]
    b.push(3, 8); b.push(1, 8); b.push(AUD_PID, 16);
    b.push(5, 8); b.push(0x81, 8); b.push(3, 4); b.push(3, 4); b.push(0, 24);
    return b.bytes();
  })();
  const stnPadded = cat(u16(stn.length), stn);

  const pw = new BW();
  pw.push(0, 11);     // reserved
  pw.push(0, 1);      // is_multi_angle
  pw.push(1, 4);      // connection_condition
  pw.push(0, 8);      // stc_id
  pw.push(IN_TIME, 32);  // in_time
  pw.push(OUT_TIME, 32); // out_time
  pw.push(0, 64);     // uo mask
  pw.push(0, 1); pw.push(0, 7); // random_access + reserved
  pw.push(0, 8);      // still_mode
  pw.push(0, 16);     // reserved
  const piContent = cat(ascii("00000"), ascii("M2TS"), pw.bytes());
  const playitem = cat(u16(piContent.length), piContent);

  const plBody = cat(u16(0), u16(1), u16(0), playitem, stnPadded);
  const LIST_POS = 64;

  const appBits2 = new BW();
  appBits2.push(0, 8);   // reserved
  appBits2.push(1, 8);   // playback_type = movie
  appBits2.push(0, 16);  // reserved
  appBits2.push(0, 64);  // uo mask
  appBits2.push(0, 5);   // flags
  const appInfo = appBits2.bytes();

  const appBlock = cat(u32(appInfo.length), appInfo);
  const pre = cat(ascii("MPLS"), ascii("0200"), u32(LIST_POS), u32(0), u32(0), zeros(20), appBlock);
  const pad = zeros(Math.max(0, LIST_POS - pre.length));
  const listBlock = cat(u32(plBody.length), plBody);
  const markPos = LIST_POS + listBlock.length;
  const markBlock = cat(u32(2), u16(0));

  const buf = cat(pre, pad, listBlock, markBlock);
  buf.writeUInt32BE(markPos, 12);
  return buf;
}

// ------------------------------------------------------------------ clpi
function buildClpi(numPackets) {
  const HEADER = 40;
  const SEQ_ADDR = 190;

  const clipInfoLen = 4 + 2 + 1 + 1 + 4 + 4 + 4 + 128 + 2;
  const seqBody = (() => {
    const b = new BW();
    b.push(1, 8);               // num_atc_seq
    b.push(0, 32);              // spn_atc_start
    b.push(1, 8);               // num_stc_seq
    b.push(0, 8);               // offset_stc_id
    b.push(VID_PID, 16);        // pcr_pid
    b.push(0, 32);              // spn_stc_start
    b.push(IN_TIME, 32);        // presentation_start_time
    b.push(OUT_TIME, 32);       // presentation_end_time
    return b.bytes();
  })();
  const seq = cat(u32(seqBody.length), u8(0), seqBody);
  const PROG_ADDR = SEQ_ADDR + seq.length;

  const attr = (codingType, extra) => {
    const b = new BW();
    b.push(codingType, 8);
    b.push(extra.format, 4); b.push(extra.rate, 4);
    if (extra.lang) { b.push(0, 24); }
    else { b.push(extra.aspect ?? 3, 4); b.push(0, 2); b.push(1, 1); b.push(0, 17); }
    b.push(0, 96); // isrc 12 bytes
    return b.bytes();
  };
  const vAttr = attr(0x1b, { format: 6, rate: 1 });
  const aAttr = attr(0x81, { format: 3, rate: 3, lang: true });
  const progBody = (() => {
    const b = new BW();
    b.push(1, 8);   // num_prog
    b.push(0, 32);  // spn_program_sequence_start
    b.push(0x0100, 16); // program_map_pid
    b.push(2, 8);   // num_streams
    b.push(0, 8);   // num_groups
    b.push(VID_PID, 16); b.push(vAttr.length, 8); b.pushBytes(vAttr);
    b.push(AUD_PID, 16); b.push(aAttr.length, 8); b.pushBytes(aAttr);
    return b.bytes();
  })();
  const prog = cat(u32(progBody.length), u8(0), progBody);
  const CPI_ADDR = PROG_ADDR + prog.length;

  const clipInfo = (() => {
    const b = new BW();
    b.push(clipInfoLen - 4, 32); // ClipInfo len
    b.push(0, 16);               // reserved
    b.push(1, 8);                // clip_stream_type (movie)
    b.push(1, 8);                // application_type
    b.push(0, 31); b.push(0, 1);// reserved + is_atc_delta=0
    b.push(25000000, 32);        // ts_recording_rate
    b.push(numPackets, 32);      // num_source_packets
    b.push(0, 128 * 8);          // reserved
    b.push(0, 16);               // ts_type_info len = 0
    return b.bytes();
  })();

  const header = cat(
    ascii("HDMV"), ascii("0200"),
    u32(SEQ_ADDR), u32(PROG_ADDR), u32(CPI_ADDR), u32(0), u32(0),
    zeros(HEADER - 8 - 5 * 4),
  );
  const cpi = u32(0);
  const buf = cat(header, clipInfo, seq, prog, cpi);
  return buf;
}

// ------------------------------------------------------------------- build
rmSync(outdir, { recursive: true, force: true });
mkdirSync(join(outdir, "BDMV/PLAYLIST"), { recursive: true });
mkdirSync(join(outdir, "BDMV/CLIPINF"), { recursive: true });
mkdirSync(join(outdir, "BDMV/STREAM"), { recursive: true });

const m2ts = join(outdir, "BDMV/STREAM/00000.m2ts");
console.log("encoding test clip ...");
execFileSync("ffmpeg", [
  "-y", "-hide_banner", "-loglevel", "error",
  "-f", "lavfi", "-i", `testsrc2=size=1920x1080:rate=24000/1001`,
  "-f", "lavfi", "-i", `sine=frequency=440:sample_rate=48000`,
  "-t", String(seconds),
  "-c:v", "libx264", "-profile:v", "high", "-level", "4.1", "-pix_fmt", "yuv420p", "-b:v", "8M",
  "-c:a", "ac3", "-b:a", "192k", "-ac", "3", "-ar", "48000",
  "-streamid", `0:0x${VID_PID.toString(16)}`, "-streamid", `1:0x${AUD_PID.toString(16)}`,
  "-f", "mpegts", m2ts,
]);

const firstPts = parseFloat(execFileSync("ffprobe", ["-v", "error", "-select_streams", "v",
  "-show_entries", "packet=pts_time", "-of", "csv=p=0", "-read_intervals", "%+#1", m2ts]).toString().trim());
IN_TIME = Math.round(firstPts * 45000);
OUT_TIME = IN_TIME + DURATION_45K;
const packets = Math.floor(statSync(m2ts).size / 192);
writeFileSync(join(outdir, "BDMV/index.bdmv"), buildIndex());
writeFileSync(join(outdir, "BDMV/MovieObject.bdmv"), buildMovieObject());
const mpls = buildMpls();
writeFileSync(join(outdir, "BDMV/PLAYLIST/00000.mpls"), mpls);
writeFileSync(join(outdir, "BDMV/CLIPINF/00000.clpi"), buildClpi(packets));
console.log(`wrote BDMV to ${outdir} (${seconds}s, ${packets} TS packets)`);
