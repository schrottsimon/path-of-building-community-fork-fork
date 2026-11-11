// Compare tab: other builds next to this one, as upstream's CompareTab
// shows them. Summary: the sidebar stats of both builds with their
// differences, and the compare power report (what each of the other build's
// nodes, items, gems and config options is worth in this build).

import { useEffect, useState } from "preact/hooks";
import type { TabProps } from "../app";
import { engine } from "../app";
import type { Compare, ComparePower, CompareTree } from "../engine/types";
import { ColorText } from "./ColorText";
import { ControlView } from "./Control";
import { TreeTab } from "./TreeTab";

const views = ["Summary", "Tree"] as const;
type View = (typeof views)[number];

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

export function CompareTab(props: TabProps) {
  const { act, rev, setControl } = props;
  const [cmp, setCmp] = useState<Compare | null>(null);
  const [view, setView] = useState<View>("Summary");
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
        <nav class="compare-views">
          {views.map((v) => (
            <button key={v} class={v === view ? "active" : ""} onClick={() => setView(v)}>
              {v}
            </button>
          ))}
        </nav>
      ) : null}
      {cmp.summary && view === "Tree" ? <CompareTreeView {...props} /> : null}
      {cmp.summary && view === "Summary" ? (
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

// Tree: the build's tree with the comparison's differences on it, or both
// trees side by side; the comparison's tree set and copying it into the build
function CompareTreeView(props: TabProps) {
  const { rev, setControl } = props;
  const [ct, setCt] = useState<CompareTree | null>(null);
  useEffect(() => {
    engine.call<CompareTree>("compareTree").then(setCt);
  }, [rev]);
  if (!ct) return <p>Loading...</p>;
  return (
    <div class="compare-tree">
      <div class="compare-controls">
        {ct.controls.map((c) => (
          <ControlView key={c.path} path={c.path!} c={c} set={setControl} label={c.path === "compare.rightSpecSelect" ? "Compared tree" : undefined} />
        ))}
        {ct.overlay ? (
          <span class="compare-legend">
            <span style={{ color: "#20e020" }}>only compared</span> <span style={{ color: "#e02020" }}>only this build</span>{" "}
            <span style={{ color: "#3060ff" }}>different mastery or jewel</span>
          </span>
        ) : null}
      </div>
      {ct.overlay ? (
        <div class="compare-trees">
          <TreeTab key="overlay" {...props} mode="overlay" />
        </div>
      ) : (
        <div class="compare-trees side">
          <TreeTab key="build" {...props} />
          <TreeTab key="compare" {...props} mode="compare" />
        </div>
      )}
    </div>
  );
}
