# Significantly AI-assisted

import bisect
import json
import plotly.graph_objects as go
from plotly.subplots import make_subplots
import pathlib as path

output_dir = path.Path("output/plots")
output_dir.mkdir(parents=True, exist_ok=True)
grouped: dict[str, list] = {}
for p in path.Path("output/aggregation").iterdir():
    if not p.is_file():
        continue
    with open(p) as f:
        data = json.load(f)
        config = data["config"]
        if grouped.get(config) is None:
            grouped[config] = []
        grouped[config].append((data["version"], data["aggregation"]["throughput"], data["aggregation"]["sorted_backpressure_values"],))

global_bp_max = 0
global_tp_max = 0
for series in grouped.values():
    for _,buckets,bp_values in series:
        for _, val in bp_values:
            global_bp_max = max(global_bp_max, val)
        for b in buckets:
            if b.get("Gauge"):
                global_tp_max = max(global_tp_max, b["Gauge"]["value"])

for config, series in grouped.items():
    fig = make_subplots(
        rows=len(series),
        cols=1,
        subplot_titles=[version for version,_,_ in series],
        shared_xaxes=True,
        specs = [[{"secondary_y":True}] for _ in series]
    )
    for i, (version, buckets, bp_values) in enumerate(series):
        times = []
        values = []
        for b in buckets:
            if b.get("Gauge"):
                times.append(b["Gauge"]["start"])
                values.append(b["Gauge"]["value"])
            else:
                continue
                times.append(b["NoEvents"]["start"])
                values.append(0)

        agg_t_vals: dict[int] = {}
        # due to a coarse clock in the benchmark, multiple values have the same t, just aggregate somehow
        # current aggregation: max
        for ts, val in bp_values:
            if agg_t_vals.get(ts) is None:
                agg_t_vals[ts] = val
            else:
                agg_t_vals[ts] = max(agg_t_vals[ts],val)

        bp_ts = sorted(agg_t_vals)
        bp_agg = [agg_t_vals[t] for t in bp_ts]

        bp_bisected = []
        for t in times:
            idx = bisect.bisect_right(bp_ts, t) - 1
            val = bp_agg[idx]
            # if val > 0.0 and t > 0.0:
            #     print(f"{t}, {val}")
            bp_bisected.append(val if idx >=0 else 0)
        

        
        fig.add_trace(go.Scatter(
            x=times,
            y=values,
            mode="lines",
            name=f"throughput"
        ), row=i+1, col=1, secondary_y=False)

        fig.add_trace(go.Bar(
            x=times,
            y=bp_bisected,
            name=f"backpressure",
            marker_color="rgba(255,0,0,0.3)",
        ), row=i+1, col=1, secondary_y=True)

    
    fig.update_layout(
        title=f"Throughput vs Time, {config}",
        hovermode="x unified",
    )
    for i in range(1, len(series) + 1):
        fig.update_layout({
            f"yaxis{i * 2 - 1}": dict(range=[0, global_tp_max * 1.05]),
            f"yaxis{i * 2}": dict(range=[0, global_bp_max * 1.05]),
        })

    plot_path = f"{output_dir}/{config}.html"
    fig.write_html(plot_path)
    print(f"wrote {config} plot to {plot_path}")
