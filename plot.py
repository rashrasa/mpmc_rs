# AI-assisted
from collections import defaultdict
import pathlib as path
import plotly.graph_objects as go
from plotly.subplots import make_subplots
import orjson
import sys

try:
    sys.argv.index("images")
    IMAGES = True
except ValueError:
    IMAGES = False


def add_ribbon(
    fig,
    row: int,
    col: int,
    times: list,
    p50: list,
    p99: list,
    p999: list,
    fill_color: str,
    line_color: str,
    line_color_faded: str,
    marker_color: str,
    name_prefix: str,
    show_legend: bool,
    visible,
    legendgroup: str = None,
):
    group = legendgroup if legendgroup is not None else name_prefix
    fig.add_trace(
        go.Scatter(
            x=times + times[::-1],
            y=p99 + p50[::-1],
            fill="toself",
            fillcolor=fill_color,
            line=dict(width=0),
            name=f"{name_prefix} p50-p99",
            legendgroup=group,
            showlegend=show_legend,
            hoverinfo="skip",
            visible=visible,
        ),
        row=row,
        col=col,
    )
    fig.add_trace(
        go.Scatter(
            x=times,
            y=p50,
            mode="lines",
            line=dict(color=line_color, width=2),
            name=f"{name_prefix} p50",
            legendgroup=group,
            showlegend=show_legend,
            visible=visible,
        ),
        row=row,
        col=col,
    )
    fig.add_trace(
        go.Scatter(
            x=times,
            y=p99,
            mode="lines",
            line=dict(color=line_color_faded, width=1.5, dash="dot"),
            name=f"{name_prefix} p99",
            legendgroup=group,
            showlegend=show_legend,
            visible=visible,
        ),
        row=row,
        col=col,
    )
    fig.add_trace(
        go.Scatter(
            x=times,
            y=p999,
            mode="markers",
            marker=dict(color=marker_color, size=4, symbol="circle"),
            name=f"{name_prefix} p999",
            legendgroup=group,
            showlegend=show_legend,
            visible=visible,
        ),
        row=row,
        col=col,
    )


COL_THROUGHPUT = 1
COL_LATENCY = 2
COL_DELAY = 3


def write_plots(row: int, fig, s: dict, config_summary: dict, show_legend: bool):
    global_bp_max = config_summary["bp_max"]
    global_tp_max = config_summary["tp_max"]
    global_lat_max = config_summary["lat_max"]
    global_delay_max = config_summary["delay_max"]

    # --- Column 1: Throughput + backpressure overlay ---
    fig.add_trace(
        go.Scatter(
            x=s["throughput"]["t"],
            y=s["throughput"]["values"],
            mode="lines",
            name="throughput",
            legendgroup="throughput",
            showlegend=show_legend,
        ),
        row=row,
        col=COL_THROUGHPUT,
        secondary_y=False,
    )
    fig.add_trace(
        go.Bar(
            x=s["backpressure"]["t"],
            y=s["backpressure"]["values"],
            name="backpressure",
            marker_color="rgba(80,80,80,0.1)",
            legendgroup="backpressure",
            showlegend=show_legend,
        ),
        row=row,
        col=COL_THROUGHPUT,
        secondary_y=True,
    )
    fig.update_yaxes(
        range=[0, global_tp_max * 1.05],
        title_text="throughput",
        row=row,
        col=COL_THROUGHPUT,
        secondary_y=False,
    )
    fig.update_yaxes(
        range=[0, global_bp_max * 1.05],
        title_text="backpressure",
        showgrid=False,
        row=row,
        col=COL_THROUGHPUT,
        secondary_y=True,
    )

    # --- Column 2: Latency ribbon + backpressure overlay ---
    fig.add_trace(
        go.Bar(
            x=s["backpressure"]["t"],
            y=s["backpressure"]["values"],
            name="backpressure",
            marker_color="rgba(80,80,80,0.1)",
            legendgroup="backpressure",
            showlegend=False,
        ),
        row=row,
        col=COL_LATENCY,
        secondary_y=True,
    )
    add_ribbon(
        fig,
        row,
        COL_LATENCY,
        s["latency"]["t"],
        s["latency"]["p50"],
        s["latency"]["p99"],
        s["latency"]["p999"],
        fill_color="rgba(99,153,34,0.18)",
        line_color="rgba(99,153,34,1.0)",
        line_color_faded="rgba(99,153,34,0.55)",
        marker_color="rgba(99,153,34,0.7)",
        name_prefix="latency",
        show_legend=show_legend,
        visible=None,
    )
    fig.update_yaxes(
        range=[0, global_lat_max * 1.1],
        title_text="latency (s)",
        row=row,
        col=COL_LATENCY,
        secondary_y=False,
    )
    fig.update_yaxes(
        range=[0, global_bp_max * 1.05],
        showgrid=False,
        row=row,
        col=COL_LATENCY,
        secondary_y=True,
    )

    # --- Column 3: Send + recv delay ribbons + backpressure overlay ---
    fig.add_trace(
        go.Bar(
            x=s["backpressure"]["t"],
            y=s["backpressure"]["values"],
            name="backpressure",
            marker_color="rgba(80, 80, 80, 0.1)",
            legendgroup="backpressure",
            showlegend=False,
        ),
        row=row,
        col=COL_DELAY,
        secondary_y=True,
    )
    for (
        key,
        fill,
        line,
        faded,
        marker_color,
        prefix,
        visible,
    ) in [
        (
            "send",
            "rgba(55,138,221,0.18)",
            "rgba(55,138,221,1.0)",
            "rgba(55,138,221,0.55)",
            "rgba(55,138,221,0.7)",
            "send",
            None,
        ),
        (
            "recv",
            "rgba(211,84,126,0.18)",
            "rgba(211,84,126,1.0)",
            "rgba(211,84,126,0.55)",
            "rgba(211,84,126,0.7)",
            "recv",
            "legendonly",
        ),
    ]:
        add_ribbon(
            fig,
            row,
            COL_DELAY,
            s[key]["t"],
            s[key]["p50"],
            s[key]["p99"],
            s[key]["p999"],
            fill_color=fill,
            line_color=line,
            line_color_faded=faded,
            marker_color=marker_color,
            name_prefix=prefix,
            show_legend=show_legend,
            visible=visible,
        )
    fig.update_yaxes(
        range=[0, global_delay_max * 1.1],
        title_text="delay (s)",
        row=row,
        col=COL_DELAY,
        secondary_y=False,
    )
    fig.update_yaxes(
        range=[0, global_bp_max * 1.05],
        showgrid=False,
        row=row,
        col=COL_DELAY,
        secondary_y=True,
    )


def handle_config(config, agg_dir, output_dir, config_summary):
    versions = sorted(config_summary["versions"])
    n_rows = len(versions)
    column_titles = ["Throughput Rate", "Latency", "Send / Recv Delay"]
    column_subtitles = {
        column_titles[0]: ["estimated number of requests handled per second"],
        column_titles[1]: ["how long until a message is received"],
        column_titles[2]: [
            "how long until a send/recv call returns",
            "recv delays include waiting on an empty queue and may not be relevant. click (legend) to re-enable",
        ],
    }
    specs = [
        [{"secondary_y": True}, {"secondary_y": True}, {"secondary_y": True}]
        for _ in range(n_rows)
    ]
    subplot_titles = []
    for i in range(0, 3 * n_rows):
        subplot_titles.append(versions[i // 3])
    fig = make_subplots(
        rows=n_rows,
        cols=3,
        column_titles=column_titles,
        subplot_titles=subplot_titles,
        row_heights=[200 for _ in range(n_rows)],
        vertical_spacing=0.25 / max(n_rows, 1),
        horizontal_spacing=0.06,
        specs=specs,
    )

    for i, version in enumerate(versions):
        with open(agg_dir / f"{version}_{config}.json") as f:
            run = orjson.loads(f.read())["summary"]
        write_plots(i + 1, fig, run, config_summary, show_legend=(i == 0))

    annotations_to_add = []

    def for_annotation(a):
        if a.text in column_titles:
            a.update(
                y=1.06,
                font_size=24,
            )
            annotations_to_add.append((a.x, a.y, 16, column_subtitles[a.text]))

    fig.for_each_annotation(for_annotation)

    for x, y, font_size, subtitles in annotations_to_add:
        for i, text in enumerate(subtitles):
            fig.add_annotation(
                xref="paper",
                yref="paper",
                xanchor="center",
                yanchor="top",
                x=x,
                y=y - i * 0.02,
                font_size=font_size - i * 4,
                opacity=1.0 - i * 0.4,
                text=text,
                showarrow=False,
            )

    fig.update_layout(hovermode="x unified", height=200 * n_rows, width=2560)

    out_path = output_dir / f"{config}.html"
    fig.write_html(out_path)
    print(f"wrote {out_path}")
    if IMAGES:
        out_path = output_dir / f"{config}.jpg"
        fig.write_image(out_path)
        print(f"wrote {out_path}")


METRIC_KEYS = ["throughput", "backpressure", "latency", "send", "recv"]

PALETTE = [
    "#1f77b4",
    "#ff7f0e",
    "#2ca02c",
    "#d62728",
    "#9467bd",
    "#8c564b",
    "#e377c2",
    "#7f7f7f",
    "#bcbd22",
    "#17becf",
]


def hex_to_rgba(hex_color: str, alpha: float) -> str:
    hex_color = hex_color.lstrip("#")
    r, g, b = (int(hex_color[i : i + 2], 16) for i in (0, 2, 4))
    return f"rgba({r},{g},{b},{alpha})"


def parse_config(config: str):
    parts = config.split("_")
    return int(parts[0]), int(parts[1])


def load_summaries(agg_dir):
    with open(agg_dir / "summary.json") as f:
        raw = orjson.loads(f.read())

    by_config = defaultdict(dict)
    for entry in raw["summaries"]:
        version = entry["version"]
        config = entry["config"]
        metrics = {k: v for k, v in entry.items() if k not in ("version", "config")}
        by_config[config][version] = metrics
    return by_config


def compute_config_summary(version_metrics: dict) -> dict:
    versions = sorted(version_metrics.keys())
    bp_max = max((m["backpressure"]["max"] or 0.0) for m in version_metrics.values())
    tp_max = max((m["throughput"]["max"] or 0.0) for m in version_metrics.values())
    lat_max = max((m["latency"]["max"] or 0.0) for m in version_metrics.values())
    delay_max = max(
        max(m["send"]["max"] or 0.0, m["recv"]["max"] or 0.0)
        for m in version_metrics.values()
    )
    return {
        "versions": versions,
        "bp_max": bp_max,
        "tp_max": tp_max,
        "lat_max": lat_max,
        "delay_max": delay_max,
    }


# --- Scalability summary figure -------------------------------------------

CATEGORIES = [
    ("thread", "Thread Scalability", "senders = receivers"),
    ("sender", "Sender Scalability", "senders(receivers=1)"),
    ("receiver", "Receiver Scalability", "receivers(senders=1)"),
]


def filter_configs_for_category(by_config, mode):
    result = []
    for config in by_config:
        senders, receivers = parse_config(config)
        if mode == "sender" and receivers == 1:
            result.append((senders, config))
        elif mode == "receiver" and senders == 1:
            result.append((receivers, config))
        elif mode == "thread" and senders == receivers:
            result.append((senders, config))
    result.sort(key=lambda t: t[0])
    return result


def build_metric_series(by_config, configs_sorted, metric, all_versions):
    has_percentiles = False
    series = {}
    for version in all_versions:
        xs, p50s, p99s, p999s, means = [], [], [], [], []
        for x_value, config in configs_sorted:
            vm = by_config[config].get(version)
            if vm is None:
                continue
            m = vm[metric]
            xs.append(x_value)
            means.append(m["mean"])
            if m["p50"] is not None:
                has_percentiles = True
                p50s.append(m["p50"])
                p99s.append(m["p99"])
                p999s.append(m["p999"])
        if xs:
            series[version] = {
                "x": xs,
                "p50": p50s,
                "p99": p99s,
                "p999": p999s,
                "mean": means,
            }
    return series, has_percentiles


def build_summary_figure(by_config, output_dir):
    all_versions = sorted({v for cfg in by_config.values() for v in cfg})
    version_color = {v: PALETTE[i % len(PALETTE)] for i, v in enumerate(all_versions)}

    n_rows = len(CATEGORIES)
    n_cols = len(METRIC_KEYS)

    fig = make_subplots(
        rows=n_rows,
        cols=n_cols,
        column_titles=[m.capitalize() for m in METRIC_KEYS],
        row_titles=[label for _, label, _ in CATEGORIES],
        horizontal_spacing=0.04,
        vertical_spacing=0.09,
    )

    legend_shown = set()

    for row_idx, (mode, _, x_label) in enumerate(CATEGORIES, start=1):
        configs_sorted = filter_configs_for_category(by_config, mode)

        for col_idx, metric in enumerate(METRIC_KEYS, start=1):
            if not configs_sorted:
                fig.add_annotation(
                    text="no matching configs",
                    x=0.5,
                    y=0.5,
                    xref="x domain",
                    yref="y domain",
                    showarrow=False,
                    row=row_idx,
                    col=col_idx,
                )
                continue

            series, has_percentiles = build_metric_series(
                by_config, configs_sorted, metric, all_versions
            )

            for version, s in series.items():
                if not s["x"]:
                    continue
                color = version_color[version]
                show_legend = version not in legend_shown
                if show_legend:
                    legend_shown.add(version)

                if has_percentiles:
                    add_ribbon(
                        fig,
                        row_idx,
                        col_idx,
                        s["x"],
                        s["p50"],
                        s["p99"],
                        s["p999"],
                        fill_color=hex_to_rgba(color, 0.18),
                        line_color=hex_to_rgba(color, 1.0),
                        line_color_faded=hex_to_rgba(color, 0.55),
                        marker_color=hex_to_rgba(color, 0.7),
                        name_prefix=version.split("_")[0],
                        show_legend=show_legend,
                        visible=None,
                        legendgroup=version,
                    )
                else:
                    # metric has no percentiles (e.g. backpressure/throughput) -> plot mean only
                    fig.add_trace(
                        go.Scatter(
                            x=s["x"],
                            y=s["mean"],
                            mode="lines+markers",
                            line=dict(color=hex_to_rgba(color, 1.0), width=2),
                            marker=dict(size=5),
                            name=f"{version}",
                            legendgroup=version,
                            showlegend=show_legend,
                        ),
                        row=row_idx,
                        col=col_idx,
                    )

            y_title = metric if col_idx == 1 else None
            fig.update_yaxes(title_text=y_title, row=row_idx, col=col_idx)
            x_title = x_label
            fig.update_xaxes(title_text=x_title, row=row_idx, col=col_idx)

    fig.update_layout(
        hovermode="x unified",
        height=300 * n_rows,
        width=2560,
        title_text="Scalability Summary",
    )

    out_path = output_dir / "summary_scalability.html"
    fig.write_html(out_path)
    print(f"wrote {out_path}")
    if IMAGES:
        out_path = output_dir / "summary_scalability.jpg"
        fig.write_image(out_path)
        print(f"wrote {out_path}")


def main():
    agg_dir = path.Path("output/aggregation")
    output_dir = path.Path("output/plots")
    output_dir.mkdir(parents=True, exist_ok=True)

    by_config = load_summaries(agg_dir)

    for config, version_metrics in by_config.items():
        config_summary = compute_config_summary(version_metrics)
        handle_config(config, agg_dir, output_dir, config_summary)

    build_summary_figure(by_config, output_dir)


if __name__ == "__main__":
    main()
