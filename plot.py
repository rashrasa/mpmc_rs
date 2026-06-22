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
        grouped[config].append((
            data["version"],
            data["aggregation"]["throughput"],
            data["aggregation"]["backpressure_values"],
            data["aggregation"]["max_backpressure"],
            data["aggregation"]["max_throughput"],
            data["aggregation"]["data_latency"],
        ))

global_bp_max = 0
global_tp_max = 0
for series in grouped.values():
    for _,_,_,bp,tp,_ in series:
        global_bp_max = max(global_bp_max, bp)
        global_tp_max = max(global_tp_max, tp)

for config, series in grouped.items():
    series.sort(key=lambda x: x[0] )
    fig = make_subplots(
        rows=len(series),
        cols=1,
        subplot_titles=[version for version,_,_,_,_,_ in series],
        specs = [[{"secondary_y":True}] for _ in series]
    )

    latency_fig = make_subplots(
        rows=len(series),
        cols=1,
        subplot_titles=[version for version,_,_,_,_,_ in series],
        specs = [[{"secondary_y":True}] for _ in series]
    )

    global_lat_max = 0.0
    for _,_,_,_,_,lat_buckets in series:
        for b in lat_buckets:
            d = b.get("Distribution")
            if d:
                global_lat_max = max(global_lat_max, d["p99"])

    for i, (version, buckets, bp_values, _, _, lat_buckets) in enumerate(series):
        times = []
        l_p50_vals, l_p90_vals, l_p99_vals, l_p999_vals = [], [], [], []
        l_min_vals, l_max_vals = [],[]
        l_times = []
        values = []
        for b in buckets:
            if b.get("Gauge"):
                times.append(b["Gauge"]["start"])
                values.append(b["Gauge"]["value"])
        for b in lat_buckets:
            if b.get("Distribution"):
                d = b["Distribution"]
                l_times.append(d["start"])
                l_min_vals.append(d["min"])
                l_max_vals.append(d["max"])
                l_p50_vals.append(d["p50"])
                l_p90_vals.append(d["p90"])
                l_p99_vals.append(d["p99"])
                l_p999_vals.append(d["p999"])


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
        l_bp_bisected = []
        for t in l_times:
            idx = bisect.bisect_right(bp_ts, t) - 1
            l_bp_bisected.append(bp_agg[idx] if idx >= 0 else 0)

        show_legend = (i == 0)

        latency_fig.add_trace(go.Scatter(
            x=l_times + l_times[::-1],
            y=l_p90_vals + l_p50_vals[::-1],
            fill="toself",
            fillcolor="rgba(99, 153, 34, 0.18)",
            line=dict(width=0),
            name="p50–p90",
            showlegend=show_legend,
            hoverinfo="skip",
        ), row=i + 1, col=1, secondary_y=False)
 
        latency_fig.add_trace(go.Scatter(
            x=l_times + l_times[::-1],
            y=l_p99_vals + l_p90_vals[::-1],
            fill="toself",
            fillcolor="rgba(99, 153, 34, 0.10)",
            line=dict(width=0),
            name="p90–p99",
            showlegend=show_legend,
            hoverinfo="skip",
        ), row=i + 1, col=1, secondary_y=False)
 
        # p50 line
        latency_fig.add_trace(go.Scatter(
            x=l_times,
            y=l_p50_vals,
            mode="lines",
            line=dict(color="rgba(99, 153, 34, 1.0)", width=2),
            name="p50",
            showlegend=show_legend,
        ), row=i + 1, col=1, secondary_y=False)
 
        # p99 line
        latency_fig.add_trace(go.Scatter(
            x=l_times,
            y=l_p99_vals,
            mode="lines",
            line=dict(color="rgba(99, 153, 34, 0.55)", width=1.5, dash="dot"),
            name="p99",
            showlegend=show_legend,
        ), row=i + 1, col=1, secondary_y=False)
 
        # p999 dots (sparse, high-signal tail)
        latency_fig.add_trace(go.Scatter(
            x=l_times,
            y=l_p999_vals,
            mode="markers",
            marker=dict(color="rgba(186, 117, 23, 0.7)", size=4, symbol="circle"),
            name="p999",
            showlegend=show_legend,
        ), row=i + 1, col=1, secondary_y=False)
 
        # Backpressure bar (secondary axis)
        latency_fig.add_trace(go.Bar(
            x=l_times,
            y=l_bp_bisected,
            name="backpressure",
            marker_color="rgba(255, 80, 80, 0.25)",
            showlegend=show_legend,
        ), row=i + 1, col=1, secondary_y=True)


        
        fig.add_trace(go.Scatter(
            x=times,
            y=values,
            mode="lines",
            name=f"estimated throughput rate"
        ), row=i+1, col=1, secondary_y=False)

        fig.add_trace(go.Bar(
            x=times,
            y=bp_bisected,
            name=f"backpressure",
            marker_color="rgba(255,0,0,0.3)",
        ), row=i+1, col=1, secondary_y=True)

    
    fig.update_layout(
        title=f"Estimated Throughput Rate vs Time, {config}",
        hovermode="x unified",
        height=500 * len(series),
    )

    latency_fig.update_layout(
        title=f"Data Latency Distributions vs Time — {config}",
        hovermode="x unified",
        height=480 * len(series),
        legend=dict(orientation="h", yanchor="bottom", y=1.02, xanchor="right", x=1),
    )

    for i in range(1, len(series) + 1):
        fig.update_layout({
            f"yaxis{i * 2 - 1}": dict(range=[0, global_tp_max * 1.05]),
            f"yaxis{i * 2}": dict(range=[0, global_bp_max * 1.05]),
        })
        primary_axis = f"yaxis{i * 2 - 1}" if i > 1 else "yaxis"
        secondary_axis = f"yaxis{i * 2}" if i > 1 else "yaxis2"
        latency_fig.update_layout({
            primary_axis: dict(
                range=[0, global_lat_max * 1.1],
                title="latency (s)",
            ),
            secondary_axis: dict(
                range=[0, global_bp_max * 1.05],
                title="backpressure",
                showgrid=False,
            ),
        })


    plot_path = f"{output_dir}/{config}.html"
    fig.write_html(plot_path)
    print(f"wrote {config} plot to {plot_path}")

    l_plot_path = f"{output_dir}/{config}_latency.html"
    latency_fig.write_html(l_plot_path)
    print(f"wrote {config} latency plot to {l_plot_path}")
