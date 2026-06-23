# Significantly AI-assisted, some copy-pasting

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
            data["aggregation"]["send_delay"],
            data["aggregation"]["recv_delay"],
        ))

global_bp_max = 0
global_tp_max = 0
for series in grouped.values():
    for _,_,_,bp,tp,_,_,_ in series:
        global_bp_max = max(global_bp_max, bp)
        global_tp_max = max(global_tp_max, tp)

for config, series in grouped.items():
    series.sort(key=lambda x: x[0])

    fig = make_subplots(
        rows=len(series),
        cols=1,
        subplot_titles=[version for version,_,_,_,_,_,_,_ in series],
        specs=[[{"secondary_y": True}] for _ in series]
    )

    latency_fig = make_subplots(
        rows=len(series) * 2,
        cols=1,
        subplot_titles=[
            title
            for version,_,_,_,_,_,_,_ in series
            for title in (version, f"{version} - send/recv delay")
        ],
        specs=[[{"secondary_y": True}], [{"secondary_y": False}]] * len(series),
        vertical_spacing=0.04,
    )

    global_lat_max = 0.0
    global_delay_max = 0.0
    for _,_,_,_,_,lat_buckets,send_buckets,recv_buckets in series:
        for b in lat_buckets:
            d = b.get("Distribution")
            if d:
                global_lat_max = max(global_lat_max, d["max"])
        for b in send_buckets + recv_buckets:
            d = b.get("Distribution")
            if d:
                global_delay_max = max(global_delay_max, d["max"])

    for i, (version, buckets, bp_values, _, _, lat_buckets, send_buckets, recv_buckets) in enumerate(series):
        times = []
        values = []
        for b in buckets:
            if b.get("Gauge"):
                times.append(b["Gauge"]["start"])
                values.append(b["Gauge"]["value"])

        l_times = []
        l_p50_vals, l_p90_vals, l_p99_vals, l_p999_vals = [], [], [], []
        for b in lat_buckets:
            d = b.get("Distribution")
            if d:
                l_times.append(d["start"])
                l_p50_vals.append(d["p50"])
                l_p90_vals.append(d["p90"])
                l_p99_vals.append(d["p99"])
                l_p999_vals.append(d["p999"])

        s_times = []
        s_p50_vals, s_p90_vals, s_p99_vals, s_p999_vals = [], [], [], []
        for b in send_buckets:
            d = b.get("Distribution")
            if d:
                s_times.append(d["start"])
                s_p50_vals.append(d["p50"])
                s_p90_vals.append(d["p90"])
                s_p99_vals.append(d["p99"])
                s_p999_vals.append(d["p999"])

        r_times = []
        r_p50_vals, r_p90_vals, r_p99_vals, r_p999_vals = [], [], [], []
        for b in recv_buckets:
            d = b.get("Distribution")
            if d:
                r_times.append(d["start"])
                r_p50_vals.append(d["p50"])
                r_p90_vals.append(d["p90"])
                r_p99_vals.append(d["p99"])
                r_p999_vals.append(d["p999"])

        agg_t_vals: dict = {}
        for ts, val in bp_values:
            agg_t_vals[ts] = max(agg_t_vals.get(ts, 0), val)
        bp_ts = sorted(agg_t_vals)
        bp_agg = [agg_t_vals[t] for t in bp_ts]

        def bisect_bp(ts_list):
            result = []
            for t in ts_list:
                idx = bisect.bisect_right(bp_ts, t) - 1
                result.append(bp_agg[idx] if idx >= 0 else 0)
            return result

        bp_bisected   = bisect_bp(times)
        l_bp_bisected = bisect_bp(l_times)

        show_legend = (i == 0)

        fig.add_trace(go.Scatter(
            x=times, y=values, mode="lines",
            name="estimated throughput rate",
            showlegend=show_legend,
        ), row=i + 1, col=1, secondary_y=False)
        fig.add_trace(go.Bar(
            x=times, y=bp_bisected,
            name="backpressure",
            marker_color="rgba(255,0,0,0.3)",
            showlegend=show_legend,
        ), row=i + 1, col=1, secondary_y=True)

        lat_row = i * 2 + 1

        def add_ribbon(fig, row, times, p50, p90, p99, p999,
                       fill_color_inner, fill_color_outer, line_color,
                       show_legend, name_prefix):
            fig.add_trace(go.Scatter(
                x=times + times[::-1], y=p90 + p50[::-1],
                fill="toself", fillcolor=fill_color_inner,
                line=dict(width=0),
                name=f"{name_prefix} p50-p90",
                showlegend=show_legend, hoverinfo="skip",
            ), row=row, col=1)
            fig.add_trace(go.Scatter(
                x=times + times[::-1], y=p99 + p90[::-1],
                fill="toself", fillcolor=fill_color_outer,
                line=dict(width=0),
                name=f"{name_prefix} p90-p99",
                showlegend=show_legend, hoverinfo="skip",
            ), row=row, col=1)
            fig.add_trace(go.Scatter(
                x=times, y=p50, mode="lines",
                line=dict(color=line_color, width=2),
                name=f"{name_prefix} p50",
                showlegend=show_legend,
            ), row=row, col=1)
            fig.add_trace(go.Scatter(
                x=times, y=p99, mode="lines",
                line=dict(color=line_color.replace("1.0", "0.55"), width=1.5, dash="dot"),
                name=f"{name_prefix} p99",
                showlegend=show_legend,
            ), row=row, col=1)
            fig.add_trace(go.Scatter(
                x=times, y=p999, mode="markers",
                marker=dict(color="rgba(186, 117, 23, 0.7)", size=4, symbol="circle"),
                name=f"{name_prefix} p999",
                showlegend=show_legend,
            ), row=row, col=1)

        latency_fig.add_trace(go.Bar(
            x=l_times, y=l_bp_bisected,
            name="backpressure",
            marker_color="rgba(255, 80, 80, 0.25)",
            showlegend=show_legend,
        ), row=lat_row, col=1, secondary_y=True)

        delay_row = i * 2 + 2

        add_ribbon(
            latency_fig, lat_row,
            l_times, l_p50_vals, l_p90_vals, l_p99_vals, l_p999_vals,
            fill_color_inner="rgba(99, 153, 34, 0.18)",
            fill_color_outer="rgba(99, 153, 34, 0.10)",
            line_color="rgba(99, 153, 34, 1.0)",
            show_legend=show_legend,
            name_prefix="data latency",
        )
        add_ribbon(
            latency_fig, delay_row,
            s_times, s_p50_vals, s_p90_vals, s_p99_vals, s_p999_vals,
            fill_color_inner="rgba(55, 138, 221, 0.18)",
            fill_color_outer="rgba(55, 138, 221, 0.10)",
            line_color="rgba(55, 138, 221, 1.0)",
            show_legend=show_legend,
            name_prefix="send delay",
        )
        add_ribbon(
            latency_fig, delay_row,
            r_times, r_p50_vals, r_p90_vals, r_p99_vals, r_p999_vals,
            fill_color_inner="rgba(211, 84, 126, 0.18)",
            fill_color_outer="rgba(211, 84, 126, 0.10)",
            line_color="rgba(211, 84, 126, 1.0)",
            show_legend=show_legend,
            name_prefix="recv delay",
        )
    fig.update_layout(
        title=f"Estimated Throughput Rate vs Time, {config}",
        hovermode="x unified",
        height=500 * len(series),
    )
    latency_fig.update_layout(
        title=f"Latency Distributions vs Time — {config}",
        hovermode="x unified",
        height=400 * len(series) * 2,
        legend=dict(orientation="h", yanchor="bottom", y=1.02, xanchor="right", x=1),
    )

    for i in range(1, len(series) + 1):
        fig.update_layout({
            f"yaxis{i * 2 - 1}": dict(range=[0, global_tp_max * 1.05]),
            f"yaxis{i * 2}": dict(range=[0, global_bp_max * 1.05]),
        })

    total_lat_rows = len(series) * 2
    for row in range(1, total_lat_rows + 1):
        is_delay_row = row % 2 == 0

        latency_fig.update_yaxes(
            range=[
                0,
                (global_delay_max if is_delay_row else global_lat_max) * 1.1,
            ],
            title_text="latency (s)",
            row=row,
            col=1,
            secondary_y=False,
        )

        if not is_delay_row:
            latency_fig.update_yaxes(
                range=[0, global_bp_max * 1.05],
                title_text="backpressure",
                showgrid=False,
                row=row,
                col=1,
                secondary_y=True,
            )
    
    for row in range(1, len(series) + 1):
        fig.update_yaxes(
            range=[0, global_tp_max * 1.05],
            row=row,
            col=1,
            secondary_y=False,
        )

        fig.update_yaxes(
            range=[0, global_bp_max * 1.05],
            row=row,
            col=1,
            secondary_y=True,
        )

    plot_path = f"{output_dir}/{config}.html"
    fig.write_html(plot_path)
    print(f"wrote {config} plot to {plot_path}")

    l_plot_path = f"{output_dir}/{config}_latency.html"
    latency_fig.write_html(l_plot_path)
    print(f"wrote {config} latency plot to {l_plot_path}")