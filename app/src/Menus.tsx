import { useEffect, useMemo, useState } from "react";

import { api, type DiscMenu, type MenuEntry, type NavTarget } from "./api";

function fmtDur(secs: number): string {
  const s = Math.max(0, Math.round(secs));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  return `${h}:${String(m).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`;
}

function navLabel(t?: NavTarget | null): string {
  if (!t) return "none";
  const kind = t.kind === "hdmv" ? "HDMV" : t.kind === "bdj" ? "BD-J" : t.kind;
  if (t.name) return `${kind} ${t.name}`;
  if (t.object != null) return `${kind} object ${t.object}`;
  return kind;
}

function slug(s: string): string {
  return s.replace(/[^A-Za-z0-9 _.-]+/g, "-").replace(/\s+/g, " ").trim() || "menu";
}

type Props = {
  source: string;
  /** True while a rip is running: keep off the drive. */
  busy?: boolean;
  onError: (msg: string) => void;
  onToast: (msg: string) => void;
};

/**
 * Disc menu documents: parse the inserted disc's title menu, edit names, the
 * main feature and chapter labels, save it as a template, and apply a template
 * to a different disc.
 */
export function Menus({ source, busy: driveBusy, onError, onToast }: Props) {
  const [entries, setEntries] = useState<MenuEntry[]>([]);
  const [pick, setPick] = useState("");
  const [doc, setDoc] = useState<DiscMenu | null>(null);
  const [name, setName] = useState("");
  const [applyName, setApplyName] = useState("");
  const [sel, setSel] = useState<number | null>(null);
  const [dirty, setDirty] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);

  const refresh = async () => {
    try {
      setEntries(await api.menuList());
    } catch (e) {
      onError(String(e));
    }
  };

  useEffect(() => {
    void refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const selTitle = useMemo(
    () => doc?.titles.find((t) => t.index === sel) ?? null,
    [doc, sel],
  );

  const patchTitle = (index: number, patch: Partial<DiscMenu["titles"][number]>) => {
    setDoc((d) =>
      d
        ? { ...d, titles: d.titles.map((t) => (t.index === index ? { ...t, ...patch } : t)) }
        : d,
    );
    setDirty(true);
  };

  const setMain = (index: number) => {
    setDoc((d) =>
      d ? { ...d, titles: d.titles.map((t) => ({ ...t, main: t.index === index })) } : d,
    );
    setDirty(true);
  };

  const setChapter = (index: number, chapter: number, text: string) => {
    setDoc((d) =>
      d
        ? {
            ...d,
            titles: d.titles.map((t) => {
              if (t.index !== index) return t;
              const names = [...(t.chapter_names ?? [])];
              names[chapter] = text;
              return { ...t, chapter_names: names };
            }),
          }
        : d,
    );
    setDirty(true);
  };

  const parseDisc = async () => {
    if (!source) {
      onError("no disc selected");
      return;
    }
    setBusy("parse");
    try {
      const d = await api.menuParse(source);
      const suggested = slug(d.label || "disc");
      setDoc(d);
      setName(suggested);
      setApplyName(`${suggested}-applied`);
      setSel(d.titles.find((t) => t.main)?.index ?? d.titles[0]?.index ?? null);
      setDirty(true);
      onToast(`parsed ${d.titles.length} titles`);
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(null);
    }
  };

  const loadTemplate = async () => {
    if (!pick) return;
    setBusy("load");
    try {
      const d = await api.menuLoad(pick);
      setDoc(d);
      setName(pick);
      setApplyName(`${pick}-applied`);
      setSel(d.titles.find((t) => t.main)?.index ?? d.titles[0]?.index ?? null);
      setDirty(false);
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(null);
    }
  };

  const removeTemplate = async () => {
    if (!pick) return;
    setBusy("remove");
    try {
      await api.menuRemove(pick);
      if (name === pick) {
        setDoc(null);
        setName("");
      }
      setPick("");
      await refresh();
      onToast(`removed ${pick}`);
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(null);
    }
  };

  const save = async () => {
    if (!doc || !name) return;
    setBusy("save");
    try {
      const path = await api.menuSave(name, doc);
      setDirty(false);
      await refresh();
      onToast(`saved ${path.split("/").pop()}`);
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(null);
    }
  };

  const apply = async () => {
    if (!doc || !name || !applyName) return;
    if (applyName === name) {
      onError("choose a different name for the applied copy");
      return;
    }
    setBusy("apply");
    try {
      // Persist edits first: menu_apply reads the template from disk.
      await api.menuSave(name, doc);
      const d = await api.menuApply(name, source, applyName);
      setDoc(d);
      setName(applyName);
      setApplyName(`${applyName}-applied`);
      setDirty(false);
      await refresh();
      onToast(`applied to this disc as ${applyName}`);
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(null);
    }
  };

  const nav = doc?.navigation;

  return (
    <section className="card">
      <div className="row between">
        <h2>Disc menu</h2>
        <span className="sub">parse, edit, save, reuse on another disc</span>
      </div>

      <div className="row wrap" style={{ marginTop: 10, gap: 8 }}>
        <button onClick={parseDisc} disabled={!!busy || !!driveBusy || !source}>
          {busy === "parse" ? "Parsing…" : "Parse this disc"}
        </button>
        <select
          value={pick}
          onChange={(e) => setPick(e.target.value)}
          style={{ minWidth: 200 }}
        >
          <option value="">saved templates…</option>
          {entries.map((e) => (
            <option key={e.name} value={e.name}>
              {e.name} · {e.titles} titles
            </option>
          ))}
        </select>
        <button className="ghost sm" onClick={loadTemplate} disabled={!pick || !!busy}>
          Load
        </button>
        <button className="ghost sm" onClick={removeTemplate} disabled={!pick || !!busy}>
          Delete
        </button>
      </div>

      {!doc && (
        <p className="hint" style={{ marginTop: 10 }}>
          Parses the navigation table from <span className="mono">BDMV/index.bdmv</span> (first
          playback, top menu, every title's movie/BD-J object) plus the playable titles and
          chapters. Save it as a template and apply it to another disc.
        </p>
      )}

      {doc && (
        <>
          <div className="row wrap" style={{ gap: 6, marginTop: 12 }}>
            <span className="pill good">
              <span className="dot" />
              {doc.label || "disc"}
            </span>
            {doc.applied_from && <span className="pill">from {doc.applied_from}</span>}
            {nav && <span className="pill">first play {navLabel(nav.first_playback)}</span>}
            {nav && <span className="pill">top menu {navLabel(nav.top_menu)}</span>}
            {nav && <span className="pill">{nav.titles.length} nav entries</span>}
            <span className="pill">{doc.titles.length} titles</span>
          </div>
          {doc.note && (
            <p className="hint" style={{ marginTop: 8 }}>
              {doc.note}
            </p>
          )}

          {doc.titles.length > 0 && (
            <div
              style={{
                display: "grid",
                gridTemplateColumns: "minmax(320px, 1fr) minmax(280px, 1fr)",
                gap: 16,
                marginTop: 14,
              }}
            >
              <div>
                <div className="row between">
                  <b>Titles</b>
                  <span className="sub">click to edit chapters</span>
                </div>
                <div className="titles" style={{ maxHeight: 340, overflow: "auto", marginTop: 6 }}>
                  {doc.titles.map((t) => (
                    <div
                      key={t.index}
                      className={`title ${sel === t.index ? "on" : ""}`}
                      onClick={() => setSel(t.index)}
                    >
                      <span className="idx">#{t.index}</span>
                      <div className="grow">
                        <div className="row" style={{ gap: 6 }}>
                          <b>Playlist {String(t.playlist).padStart(5, "0")}</b>
                          {t.main && <span className="badge">main</span>}
                          {t.resolution && <span className="badge">{t.resolution}</span>}
                        </div>
                        <div className="meta">
                          {fmtDur(t.duration_secs)} · {t.chapters} chapters
                          {t.audio_streams ? ` · audio ${t.audio_streams}` : ""}
                        </div>
                        <input
                          className="grow"
                          value={t.name ?? ""}
                          placeholder="title name"
                          onClick={(e) => e.stopPropagation()}
                          onChange={(e) => patchTitle(t.index, { name: e.target.value })}
                          style={{ marginTop: 6, width: "100%" }}
                          aria-label={`Name for title ${t.index}`}
                        />
                      </div>
                      <button
                        className="ghost sm"
                        aria-pressed={t.main}
                        onClick={(e) => {
                          e.stopPropagation();
                          setMain(t.index);
                        }}
                      >
                        {t.main ? "main" : "make main"}
                      </button>
                    </div>
                  ))}
                </div>
              </div>

              <div>
                <div className="row between">
                  <b>Chapters</b>
                  <span className="sub">
                    {selTitle
                      ? `title #${selTitle.index} · ${selTitle.chapter_names?.length ?? 0}`
                      : "select a title"}
                  </span>
                </div>
                {!selTitle && <p className="hint">Select a title to edit its chapter labels.</p>}
                {selTitle && (selTitle.chapter_names?.length ?? 0) === 0 && (
                  <p className="hint">This title has no chapter marks.</p>
                )}
                {selTitle && (selTitle.chapter_names?.length ?? 0) > 0 && (
                  <div
                    style={{
                      display: "grid",
                      gap: 6,
                      marginTop: 8,
                      maxHeight: 340,
                      overflow: "auto",
                    }}
                  >
                    {selTitle.chapter_names?.map((label, i) => (
                      <div className="row" key={i} style={{ gap: 8 }}>
                        <span className="sub mono" style={{ width: 24 }}>
                          {i + 1}
                        </span>
                        <input
                          className="grow"
                          value={label}
                          onChange={(e) => setChapter(selTitle.index, i, e.target.value)}
                          aria-label={`Chapter ${i + 1} label`}
                        />
                      </div>
                    ))}
                  </div>
                )}
              </div>
            </div>
          )}

          {nav && nav.titles.length > 0 && (
            <details className="group" style={{ marginTop: 14 }}>
              <summary>Navigation table ({nav.titles.length})</summary>
              <div
                className="sub mono"
                style={{ marginTop: 8, maxHeight: 180, overflow: "auto" }}
              >
                {nav.titles.map((t, i) => (
                  <div key={i}>
                    #{String(i).padStart(2, "0")} {navLabel(t)}
                  </div>
                ))}
              </div>
            </details>
          )}

          <div className="row wrap" style={{ marginTop: 14, gap: 10, alignItems: "flex-end" }}>
            <label className="field" style={{ minWidth: 220 }}>
              Template name
              <input
                className="mono"
                value={name}
                onChange={(e) => {
                  setName(e.target.value);
                  setDirty(true);
                }}
                placeholder="my-disc"
              />
            </label>
            <button
              className="primary"
              onClick={save}
              disabled={!name || !!busy || !dirty}
              title={dirty ? "Write the template to disk" : "No unsaved changes"}
            >
              {busy === "save" ? "Saving…" : dirty ? "Save template" : "Saved"}
            </button>
            <label className="field" style={{ minWidth: 220 }}>
              Apply to this disc, save as
              <input
                className="mono"
                value={applyName}
                onChange={(e) => setApplyName(e.target.value)}
                placeholder={`${name || "template"}-applied`}
              />
            </label>
            <button
              className="ghost"
              onClick={apply}
              disabled={!name || !applyName || applyName === name || !!busy || !!driveBusy || !source}
              title="Match this template's titles to the inserted disc and save the result"
            >
              {busy === "apply" ? "Applying…" : "Apply"}
            </button>
          </div>
        </>
      )}
    </section>
  );
}
