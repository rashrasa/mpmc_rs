# Some AI-assistance

from collections import defaultdict
import pathlib as path
import plotly.graph_objects as go
from plotly.subplots import make_subplots
import orjson
import threading


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
):
    fig.add_trace(
        go.Scatter(
            x=times + times[::-1],
            y=p99 + p50[::-1],
            fill="toself",
            fillcolor=fill_color,
            line=dict(width=0),
            name=f"{name_prefix} p50-p99",
            legendgroup=name_prefix,
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
            legendgroup=name_prefix,
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
            legendgroup=name_prefix,
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
            legendgroup=name_prefix,
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
    global_delay_max = config_summary["delay_max"]  # NEW: add to config summary

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
    out_path = output_dir / f"{config}.jpg"
    fig.write_image(out_path)
    print(f"wrote {out_path}")


def main():
    agg_dir = path.Path("output/aggregation")
    output_dir = path.Path("output/plots")
    output_dir.mkdir(parents=True, exist_ok=True)
    with open(agg_dir / "summary.json") as f:
        summary = orjson.loads(f.read())
    configs: dict = summary["configs"]

    threads: list[threading.Thread] = []
    for config, config_summary in configs.items():
        t = threading.Thread(
            target=handle_config, args=(config, agg_dir, output_dir, config_summary)
        )
        t.start()
        threads.append(t)

    for t in threads:
        t.join()


if __name__ == "__main__":
    main()
