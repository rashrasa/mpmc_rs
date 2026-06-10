import json
import plotly.graph_objects as go
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
        if grouped.get(config) == None:
            grouped[config] = []
        grouped[config].append((data["version"], data["aggregation"]["throughput"]))

for config, series in grouped.items():
    fig = go.Figure()
    for (version, buckets) in series:
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
        fig.add_trace(go.Scatter(x=times, y=values, mode="lines", name=version))
    
    fig.update_layout(
        title=f"Throughput vs Time, {config}",
        xaxis_title="Time (s)",
        yaxis_title="Throughput"
    )
    plot_path = f"{output_dir}/{config}.html"
    fig.write_html(plot_path)
    print(f"wrote {config} plot to {plot_path}")
