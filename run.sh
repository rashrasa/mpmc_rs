set -e

benchmark=false
aggregate=false
plot=false

if [ $# -eq 0 ]; then
    benchmark=true
    aggregate=true
    plot=true
fi

for arg in "$@"; do
    case "$arg" in
        bench) benchmark=true ;;
        benchmark) benchmark=true ;;
        aggregate) aggregate=true ;;
        plot) plot=true ;;
        *) echo "valid arguments are: bench, benchmark, aggregate, plot"; exit 1 ;;
    esac
done

source "$HOME/.cargo/env"

if $benchmark; then
    echo running benchmark
    cargo run --bin benchmark --release 2>&1
fi

if $aggregate; then
    echo aggregating benchmark result
    cargo run --bin aggregate --release output/result 2>&1
fi

if $plot; then
    if ! source .venv/bin/activate; then
        echo creating python virtual env
        python3 -m venv .venv
        source .venv/bin/activate
    fi

    echo installing pip dependencies
    pip install -r requirements.txt -q

    echo plotting
    python3 plot.py 2>&1
    deactivate
fi