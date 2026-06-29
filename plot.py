# Significantly AI-assisted, some copy-pasting

import json
import pathlib as path
from line_profiler import profile
import plotly.graph_objects as go
from plotly.subplots import make_subplots


@profile
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
            name=f"{name_prefix} p50–p99",
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


class ConfigFigures:
    """Holds the two growing figures for one config value."""

    def __init__(self, config: str):
        self.config = config
        self.n_rows = 0
        self.tp_fig = None
        self.lat_fig = None
        self._tp_specs: list = []
        self._lat_specs: list = []
        self._tp_titles: list = []
        self._lat_titles: list = []

    def _rebuild(self, version: str):
        """Extend subplot grids by one version's worth of rows."""
        self.n_rows += 1
        row = self.n_rows

        self._tp_titles.append(version)
        self._tp_specs.append([{"secondary_y": True}])

        self._lat_titles += [
            f"{version} — latency",
            f"{version} — send delay",
            f"{version} — recv delay",
        ]
        self._lat_specs += [
            [{"secondary_y": True}],
            [{"secondary_y": False}],
            [{"secondary_y": False}],
        ]

        tp_fig = make_subplots(
            rows=self.n_rows,
            cols=1,
            subplot_titles=self._tp_titles,
            specs=self._tp_specs,
        )
        lat_fig = make_subplots(
            rows=self.n_rows * 3,
            cols=1,
            subplot_titles=self._lat_titles,
            specs=self._lat_specs,
            vertical_spacing=0.06,
        )

        # carry over existing traces into the new figure objects
        if self.tp_fig is not None:
            for trace in self.tp_fig.data:
                r = trace.meta["row"] if trace.meta and "row" in trace.meta else None
                tp_fig.add_trace(
                    trace,
                    row=r,
                    col=1,
                    secondary_y=(
                        trace.meta.get("secondary_y", False) if trace.meta else False
                    ),
                )
        if self.lat_fig is not None:
            for trace in self.lat_fig.data:
                r = trace.meta["row"] if trace.meta and "row" in trace.meta else None
                lat_fig.add_trace(
                    trace,
                    row=r,
                    col=1,
                    secondary_y=(
                        trace.meta.get("secondary_y", False) if trace.meta else False
                    ),
                )

        self.tp_fig = tp_fig
        self.lat_fig = lat_fig
        return row

    @profile
    def append(self, version: str, s: dict, summary: dict, show_legend: bool):
        global_bp_max = summary["global_bp_max"]
        global_tp_max = summary["global_tp_max"]
        global_lat_max = summary["global_lat_max"]

        tp_row = self._rebuild(version)
        lat_row = (tp_row - 1) * 3 + 1  # latency subplot for this version

        # throughput
        self.tp_fig.add_trace(
            go.Scatter(
                x=s["t_tp"],
                y=s["throughput"],
                mode="lines",
                name="throughput",
                showlegend=show_legend,
                meta={"row": tp_row, "secondary_y": False},
            ),
            row=tp_row,
            col=1,
            secondary_y=False,
        )
        self.tp_fig.add_trace(
            go.Bar(
                x=s["t_bp"],
                y=s["backpressure"],
                name="backpressure",
                marker_color="rgba(255,0,0,0.3)",
                showlegend=show_legend,
                meta={"row": tp_row, "secondary_y": True},
            ),
            row=tp_row,
            col=1,
            secondary_y=True,
        )

        self.tp_fig.update_yaxes(
            range=[0, global_tp_max * 1.05], row=tp_row, col=1, secondary_y=False
        )
        self.tp_fig.update_yaxes(
            range=[0, global_bp_max * 1.05],
            row=tp_row,
            col=1,
            secondary_y=True,
            showgrid=False,
        )

        # latency / send / recv (rows lat_row, lat_row+1, lat_row+2)
        self.lat_fig.add_trace(
            go.Bar(
                x=s["t_bp"],
                y=s["backpressure"],
                name="backpressure",
                marker_color="rgba(255, 80, 80, 0.25)",
                showlegend=show_legend,
                meta={"row": lat_row, "secondary_y": True},
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
                lat_row + 1,
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
                lat_row + 2,
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
                self.lat_fig,
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
            self.lat_fig.update_yaxes(
                range=[0, global_lat_max * 1.1],
                title_text="latency (s)",
                row=row,
                col=1,
                secondary_y=False,
            )

        self.lat_fig.update_yaxes(
            range=[0, global_bp_max * 1.05],
            title_text="backpressure",
            showgrid=False,
            row=lat_row,
            col=1,
            secondary_y=True,
        )

    @profile
    def write(self, output_dir: path.Path, summary: dict):
        n = self.n_rows
        self.tp_fig.update_layout(
            title=f"Throughput — {self.config}",
            hovermode="x unified",
            height=500 * n,
        )
        self.lat_fig.update_layout(
            title=f"Latency — {self.config}",
            hovermode="x unified",
            height=400 * n * 3,
            legend=dict(
                orientation="h", yanchor="bottom", y=1.02, xanchor="right", x=1
            ),
        )
        self.tp_fig.write_html(output_dir / f"{self.config}_throughput.html")
        self.lat_fig.write_html(output_dir / f"{self.config}_latency.html")
        print(f"wrote {self.config}")


@profile
def main():
    agg_dir = path.Path("output/aggregation")
    output_dir = path.Path("output/plots")
    output_dir.mkdir(parents=True, exist_ok=True)

    print("loading summary")
    with open(agg_dir / "summary.json") as f:
        summary = json.load(f)

    figures: dict[str, ConfigFigures] = {}

    for p in sorted(agg_dir.iterdir()):
        if not p.is_file() or p.name == "summary.json":
            continue

        print(f"processing {p}")
        with open(p) as f:
            run = json.load(f)

        config = run["config"]
        version = run["version"]

        if config not in figures:
            figures[config] = ConfigFigures(config)

        cf = figures[config]
        cf.append(version, run["summary"], summary, show_legend=(cf.n_rows == 0))

    for cf in figures.values():
        cf.write(output_dir, summary)


if __name__ == "__main__":
    main()
