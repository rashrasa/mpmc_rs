# Some AI-assistance

from collections import defaultdict
import pathlib as path
import plotly.graph_objects as go
from plotly.subplots import make_subplots
import json
import threading


def add_ribbon(
    fig,
    row: int,
    times: list,
    p50: list,
    p99: list,
    p999: list,
    fill_color: str,
    line_color: str,
    line_color_faded: str,
    name_prefix: str,
    show_legend: bool,
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
        ),
        row=row,
        col=1,
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
        ),
        row=row,
        col=1,
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
        ),
        row=row,
        col=1,
    )
    fig.add_trace(
        go.Scatter(
            x=times,
            y=p999,
            mode="markers",
            marker=dict(color="rgba(186, 117, 23, 0.7)", size=4, symbol="circle"),
            name=f"{name_prefix} p999",
            legendgroup=name_prefix,
            showlegend=show_legend,
        ),
        row=row,
        col=1,
    )


def write_plots(
    lat_row, lat_fig, tp_row, tp_fig, s: dict, config_summary: dict, show_legend: bool
):
    global_bp_max = config_summary["bp_max"]
    global_tp_max = config_summary["tp_max"]
    global_lat_max = config_summary["lat_max"]
    # throughput
    tp_fig.add_trace(
        go.Scatter(
            x=s["t_tp"],
            y=s["throughput"],
            mode="lines",
            name="throughput",
            showlegend=show_legend,
        ),
        row=tp_row,
        col=1,
        secondary_y=False,
    )
    tp_fig.add_trace(
        go.Bar(
            x=s["t_bp"],
            y=s["backpressure"],
            name="backpressure",
            marker_color="rgba(255,0,0,0.3)",
            showlegend=show_legend,
        ),
        row=tp_row,
        col=1,
        secondary_y=True,
    )
    tp_fig.update_yaxes(
        range=[0, global_tp_max * 1.05], row=tp_row, col=1, secondary_y=False
    )
    tp_fig.update_yaxes(
        range=[0, global_bp_max * 1.05],
        row=tp_row,
        col=1,
        secondary_y=True,
        showgrid=False,
    )
    # latency / send / recv (rows lat_row, lat_row+1, lat_row+2)
    lat_fig.add_trace(
        go.Bar(
            x=s["t_bp"],
            y=s["backpressure"],
            name="backpressure",
            marker_color="rgba(255, 80, 80, 0.25)",
            showlegend=show_legend,
        ),
        row=lat_row,
        col=1,
        secondary_y=True,
    )
    for row, key_p50, key_p99, key_p999, t_key, fill, line, faded, prefix in [
        (
            lat_row,
            "latency_p50",
            "latency_p99",
            "latency_p999",
            "t_lat",
            "rgba(99,153,34,0.18)",
            "rgba(99,153,34,1.0)",
            "rgba(99,153,34,0.55)",
            "latency",
        ),
        (
            lat_row,
            "send_p50",
            "send_p99",
            "send_p999",
            "t_send",
            "rgba(55,138,221,0.18)",
            "rgba(55,138,221,1.0)",
            "rgba(55,138,221,0.55)",
            "send",
        ),
        (
            lat_row,
            "recv_p50",
            "recv_p99",
            "recv_p999",
            "t_recv",
            "rgba(211,84,126,0.18)",
            "rgba(211,84,126,1.0)",
            "rgba(211,84,126,0.55)",
            "recv",
        ),
    ]:
        add_ribbon(
            lat_fig,
            row,
            s[t_key],
            s[key_p50],
            s[key_p99],
            s[key_p999],
            fill_color=fill,
            line_color=line,
            line_color_faded=faded,
            name_prefix=prefix,
            show_legend=show_legend,
        )
        lat_fig.update_yaxes(
            range=[0, global_lat_max * 1.1],
            title_text="latency (s)",
            row=row,
            col=1,
            secondary_y=False,
        )
    lat_fig.update_yaxes(
        range=[0, global_bp_max * 1.05],
        title_text="backpressure",
        showgrid=False,
        row=lat_row,
        col=1,
        secondary_y=True,
    )


def write_figure(n, tp_fig, lat_fig, output_dir: path.Path, config):
    tp_fig.update_layout(
        hovermode="x unified",
    )
    lat_fig.update_layout(
        hovermode="x unified",
    )
    tp_fig.write_html(output_dir / f"{config}_throughput.html")
    lat_fig.write_html(output_dir / f"{config}_latency.html")
    print(f"wrote {config}")


def handle_config(config, agg_dir, output_dir, config_summary):
    versions = config_summary["versions"]
    versions = sorted(versions)
    n_rows = len(versions)
    tp_fig = make_subplots(
        rows=n_rows,
        subplot_titles=versions,
        row_heights=[400 for _ in range(len(versions))],
        vertical_spacing=0.2 / n_rows,
        shared_yaxes=True,
        specs=[[{"secondary_y": True}] for _ in range(len(versions))],
    )
    lat_fig = make_subplots(
        rows=n_rows,
        subplot_titles=versions,
        row_heights=[400 for _ in range(len(versions))],
        vertical_spacing=0.2 / n_rows,
        shared_yaxes=True,
        specs=[[{"secondary_y": True}] for _ in range(len(versions))],
    )
    for i, version in enumerate(versions):
        run = json.loads(open(agg_dir / f"{version}_{config}.json").read())["summary"]
        write_plots(i + 1, lat_fig, i + 1, tp_fig, run, config_summary, False)

    write_figure(n_rows, tp_fig, lat_fig, output_dir, config)


def main():
    agg_dir = path.Path("output/aggregation")
    output_dir = path.Path("output/plots")
    output_dir.mkdir(parents=True, exist_ok=True)

    summary = json.loads(open(agg_dir / "summary.json").read())
    configs: dict = summary["configs"]

    threads: list[threading.Thread] = []
    for config, config_summary in configs.items():
        versions = config_summary["versions"]
        t = threading.Thread(
            target=handle_config, args=(config, agg_dir, output_dir, config_summary)
        )
        t.start()
        threads.append(t)

    for t in threads:
        t.join()


if __name__ == "__main__":
    main()
