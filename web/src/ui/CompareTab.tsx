// Compare tab: other builds next to this one, as upstream's CompareTab
// shows them. Summary: the sidebar stats of both builds with their
// differences, and the compare power report (what each of the other build's
// nodes, items, gems and config options is worth in this build).

import { useEffect, useState } from "preact/hooks";
import type { TabProps } from "../app";
import { engine } from "../app";
import type { Compare, ComparePower } from "../engine/types";
import { ColorText } from "./ColorText";
import { ControlView } from "./Control";

const headerLabels: Record<string, string> = {
  "compare.compareBuildSelect": "Compare with",
  "compare.compareSpecSelect": "Tree set",
  "compare.compareSkillSetSelect": "Skill set",
  "compare.compareItemSetSelect": "Item set",
  "compare.compareConfigSetSelect": "Config set",
  "compare.cmpSocketGroup": "Skill",
  "compare.cmpSkillPart": "Part",
  "compare.cmpStageCount": "Stages",
  "compare.cmpMineCount": "Mines",
  "compare.cmpMinion": "Minion",
  "compare.cmpMinionSkill": "Minion skill",
};

export function CompareTab({ act, rev, setControl }: TabProps) {
  const [cmp, setCmp] = useState<Compare | null>(null);
  const [power, setPower] = useState<ComparePower | null>(null);
  const [powerBusy, setPowerBusy] = useState(false);
  const [input, setInput] = useState("");
  const [name, setName] = useState("");

  useEffect(() => {
    engine.call<Compare>("compare").then(setCmp);
  }, [rev]);

  // The power report is slow: run it when asked, and again after every
  // change while one is shown
  const runPower = async () => {
    setPowerBusy(true);
    try {
      setPower(await engine.call<ComparePower>("comparePower"));
    } catch (e) {
      setPower({ rows: [], status: String(e instanceof Error ? e.message : e) });
    } finally {
      setPowerBusy(false);
    }
  };
  useEffect(() => {
    if (power && cmp?.summary) void runPower();
  }, [rev]);

  const importBuild = async () => {
    const t = input.trim();
    if (!t) return;
    const args = t.startsWith("<") ? { xml: t, name } : { code: t, name };
    if ((await act("compareImport", args)) !== undefined) {
      setInput("");
      setName("");
      setPower(null);
    }
  };

  if (!cmp) return <p>Loading...</p>;
  return (
    <div class="compare-tab">
      <div class="compare-import">
        <textarea rows={2} placeholder="Build code or PoB XML to compare with" value={input} onInput={(e) => setInput((e.target as HTMLTextAreaElement).value)} />
        <input type="text" placeholder="Name (optional)" value={name} onInput={(e) => setName((e.target as HTMLInputElement).value)} />
        <button onClick={importBuild} disabled={!input.trim()}>
          Import
        </button>
        {cmp.active > 0 ? (
          <button onClick={() => void act("compareRemove", { index: cmp.active }).then(() => setPower(null))}>Remove</button>
        ) : null}
      </div>
      {cmp.controls ? (
        <div class="compare-controls">
          {cmp.controls.map((c) => (
            <ControlView key={c.path} path={c.path!} c={c} set={setControl} label={headerLabels[c.path!]} />
          ))}
        </div>
      ) : (
        <p>Import a build to compare it with this one.</p>
      )}
      {cmp.summary ? (
        <>
          <table class="compare-summary">
            <thead>
              <tr>
                <th>Stat</th>
                <th class="num">
                  <ColorText text={"^x33FF77" + cmp.primaryName} />
                </th>
                <th class="num">
                  <ColorText text={"^xFF9922" + (cmp.compareName ?? "")} />
                </th>
                <th>Difference</th>
              </tr>
            </thead>
            <tbody>
              {cmp.summary.map((r, i) =>
                r.gap ? (
                  <tr key={i} class="gap">
                    <td colSpan={4} />
                  </tr>
                ) : (
                  <tr key={i}>
                    <td>
                      <ColorText text={r.label} />
                    </td>
                    <td class="num">{r.primary}</td>
                    <td class="num">
                      <ColorText text={"^xFFD62A" + (r.compare ?? "")} />
                    </td>
                    <td>
                      <ColorText text={r.diff} />
                    </td>
                  </tr>
                ),
              )}
            </tbody>
          </table>
          <h3>Compare Power Report</h3>
          <div class="compare-controls">
            {(cmp.powerControls ?? []).map((c) => (
              <ControlView key={c.path} path={c.path!} c={c} set={setControl} label={c.path === "compare.comparePowerStatSelect" ? "Metric" : undefined} />
            ))}
            <button onClick={runPower} disabled={powerBusy}>
              {powerBusy ? "Calculating..." : "Calculate"}
            </button>
          </div>
          {power ? (
            power.rows.length ? (
              <table class="compare-power">
                <thead>
                  <tr>
                    {(power.columns ?? []).map((c, i) => (
                      <th key={i}>{c}</th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {power.rows.map((r, i) => (
                    <tr key={i}>
                      {r.cells.map((c, j) => (
                        <td key={j} class={j >= 2 ? "num" : undefined}>
                          <ColorText text={c} />
                        </td>
                      ))}
                    </tr>
                  ))}
                </tbody>
              </table>
            ) : (
              <p>{power.status}</p>
            )
          ) : null}
        </>
      ) : null}
    </div>
  );
}
