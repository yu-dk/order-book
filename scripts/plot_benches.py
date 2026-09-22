#!/usr/bin/env python3
"""Plot Criterion results from target/criterion/<op>_<workload>/<store>/<n>/new/estimates.json.

Writes plots/btree.png and plots/bucket_map.png: one subplot per operation,
titled with its time complexity. The median time in microseconds is plotted
against the book size n. O(n) subplots use linear axes, so the
distance between sizes is proportional to n. O(log n) and O(log m) subplots
plot log2(n) on the x axis instead, so a true logarithmic cost is a straight
line.

Most subplots draw all three workloads (uniform, normal, hot). The exception
is bucket_map's insert/remove/update_quantity, whose true cost is
O(log m), m the number of orders at one price (m <= n): `_uniform` and
`_normal` keep m at about 1 to 6 regardless of n, which would just look flat
and prove nothing about log(m), so those three panels draw `_hot` only, where
m = n/32 scales with n and the log2(n) x axis doubles as a log2(m) axis up to
that constant offset. The title says as much.

Also writes plots/compare.png with every store in STORES overlaid (uniform +
normal), including level_map, which has no per-store plot.
"""

from __future__ import annotations

import json
import math
import re
import sys
from collections import defaultdict
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt

ROOT = Path(__file__).resolve().parents[1]
CRITERION = ROOT / "target" / "criterion"
OUT_DIR = ROOT / "plots"

SIZES = (1_000, 4_000, 16_000, 64_000, 256_000, 1_024_000, 2_000_000)  # SIZES in benches/order_book.rs
LINEAR_TICKS = (4_000, 64_000, 1_024_000, 2_000_000)  # sparser labels for linear-axis panels (all SIZES still plotted)
WORKLOADS = ("uniform", "normal", "hot")
COMPARISON_WORKLOADS = ("uniform", "normal")  # _hot excluded: this plot compares stores, not m vs n
STORES = ("btree", "bucket_map", "level_map")
STORE_COLOR = {"btree": "#1b6ca8", "bucket_map": "#d95f02", "level_map": "#2a9d55"}  # compare.png: color = implementation
WORKLOAD_MARKER = {"uniform": "o", "normal": "s"}  # compare.png: marker shape = price distribution
GROUP = re.compile(r"^(insert|update_quantity|remove|bids|asks)_(uniform|normal|hot)$")

WORKLOAD_COLOR = {"uniform": "#1b6ca8", "normal": "#d95f02", "hot": "#2a9d55"}  # btree.png / bucket_map.png: color = workload
OP_STYLE = {"bids": ("-", "o"), "asks": ("--", "s")}  # ops that share a subplot; the rest use ("-", "o")
OP_LINESTYLE = {"bids": "-", "asks": "--"}  # compare.png: linestyle distinguishes ops sharing a panel

# Subplots per store: (title, complexity, operations drawn in it, workloads drawn in it)
PANELS = {
    "btree": (
        ("update_quantity: O(log n)", "log n", ("update_quantity",), WORKLOADS),
        ("insert: O(log n)", "log n", ("insert",), WORKLOADS),
        ("remove: O(log n)", "log n", ("remove",), WORKLOADS),
        ("bids: O(n)", "n", ("bids",), WORKLOADS),
    ),
    "bucket_map": (
        ("update_quantity: O(log m)", "log m", ("update_quantity",), ("hot",)),
        ("insert: O(log m)", "log m", ("insert",), ("hot",)),
        ("remove: O(log m)", "log m", ("remove",), ("hot",)),
        ("bids: O(n)", "n", ("bids",), WORKLOADS),
    ),
}

# Figure title per store.
SUPTITLE = {
    "btree": "btree",
    "bucket_map": "bucket_map (hot_dist, m = n/32)",
}


def load_points(store: str) -> dict[tuple[str, str], list[tuple[int, float]]]:
    """(op, workload) -> [(n, median ns)] sorted by n, for one store."""
    points: dict[tuple[str, str], list[tuple[int, float]]] = defaultdict(list)
    for estimates in CRITERION.glob(f"*/{store}/*/new/estimates.json"):
        group, size_label = estimates.parts[-5], estimates.parts[-3]
        match = GROUP.match(group)
        if match is None:
            continue
        try:
            n = int(size_label.replace("_", ""))
        except ValueError:
            continue
        if n not in SIZES:
            continue
        ns = json.loads(estimates.read_text())["median"]["point_estimate"]
        points[(match.group(1), match.group(2))].append((n, ns))

    for series in points.values():
        series.sort()
    return points


def load_comparison_points() -> dict[tuple[str, str, str], list[tuple[int, float]]]:
    """(op, workload, store) -> [(n, median ns)] sorted by n, for every store."""
    points: dict[tuple[str, str, str], list[tuple[int, float]]] = defaultdict(list)
    for store in STORES:
        for estimates in CRITERION.glob(f"*/{store}/*/new/estimates.json"):
            group, size_label = estimates.parts[-5], estimates.parts[-3]
            match = GROUP.match(group)
            if match is None or match.group(2) not in COMPARISON_WORKLOADS:
                continue
            try:
                n = int(size_label.replace("_", ""))
            except ValueError:
                continue
            if n not in SIZES:
                continue
            ns = json.loads(estimates.read_text())["median"]["point_estimate"]
            points[(match.group(1), match.group(2), store)].append((n, ns))

    for series in points.values():
        series.sort()
    return points


# Comparison subplots: (title, operations drawn in it)
COMPARISON_PANELS = (
    ("update_quantity: btree O(log n), bucket_map O(log m),\nlevel_map O(log L + log m)", ("update_quantity",)),
    ("insert: btree O(log n), bucket_map O(log m),\nlevel_map O(log L + log m)", ("insert",)),
    ("remove: btree O(log n), bucket_map O(log m),\nlevel_map O(log L + log m)", ("remove",)),
    ("bids: O(n) for all", ("bids",)),
)


def draw_comparison_panel(ax, title: str, ops: tuple[str, ...], points: dict[tuple[str, str, str], list[tuple[int, float]]]) -> None:
    log_x = ops != ("bids",)  # bids' true O(n) stays on a linear axis so the line is straight
    for op in ops:
        linestyle = OP_LINESTYLE.get(op, "-")
        for workload in COMPARISON_WORKLOADS:
            marker = WORKLOAD_MARKER[workload]
            for store in STORES:
                pts = points.get((op, workload, store))
                if not pts:
                    continue
                label = f"{store} / {workload}" if len(ops) == 1 else f"{op} / {store} / {workload}"
                xs = [math.log2(n) if log_x else n for n, _ in pts]
                ax.plot(xs, [t / 1_000.0 for _, t in pts], linestyle, marker=marker, color=STORE_COLOR[store], label=label)

    if log_x:
        ax.set_xticks([math.log2(n) for n in SIZES], [f"{n:,}" for n in SIZES])
        ax.set_xlabel("book size n (log2 scale)")
    else:
        ax.set_xticks(LINEAR_TICKS, [f"{n:,}" for n in LINEAR_TICKS])
        ax.set_xlabel("book size n")
    ax.tick_params(axis="x", labelrotation=45)
    ax.set_ylabel("median time (µs)")
    ax.set_title(title)
    ax.grid(True, which="both", linestyle=":")
    ax.legend(fontsize=6)


def plot_comparison() -> bool:
    """Writes plots/compare.png: every store overlaid, uniform + normal only."""
    points = load_comparison_points()
    if not points:
        return False
    fig, axes = plt.subplots(2, 2, figsize=(12, 9))
    for ax, (title, ops) in zip(axes.flat, COMPARISON_PANELS):
        draw_comparison_panel(ax, title, ops, points)
    fig.suptitle(f"{' vs '.join(STORES)} (uniform & normal workloads)")
    fig.tight_layout()
    dest = OUT_DIR / "compare.png"
    fig.savefig(dest, dpi=150)
    plt.close(fig)
    print(f"wrote {dest}")
    return True


def draw_panel(ax, title: str, complexity: str, ops: tuple[str, ...], workloads: tuple[str, ...], points: dict[tuple[str, str], list[tuple[int, float]]]) -> None:
    # SIZES spans 1,000 to 2,000,000 (2000x), so a linear x axis would stack
    # the small sizes on top of each other for the log-scaled complexities;
    # `n` (bids' true O(n)) stays on a linear axis so the line is straight.
    log_x = complexity != "n"
    for op in ops:
        style, marker = OP_STYLE.get(op, ("-", "o"))
        for workload in workloads:
            pts = points.get((op, workload))
            if not pts:
                continue
            label = workload if len(ops) == 1 else f"{op} / {workload}"
            xs = [math.log2(n) if log_x else n for n, _ in pts]
            ax.plot(xs, [t / 1_000.0 for _, t in pts], style, marker=marker, color=WORKLOAD_COLOR[workload], label=label)

    if log_x:
        ax.set_xticks([math.log2(n) for n in SIZES], [f"{n:,}" for n in SIZES])
        ax.set_xlabel("book size n (log2 scale)")
    else:
        ax.set_xticks(LINEAR_TICKS, [f"{n:,}" for n in LINEAR_TICKS])
        ax.set_xlabel("book size n")
    ax.tick_params(axis="x", labelrotation=45)
    ax.set_ylabel("median time (µs)")
    ax.set_title(title)
    ax.grid(True, which="both", linestyle=":")
    ax.legend(fontsize=7)


def plot_store(store: str) -> bool:
    """Writes plots/<store>.png. Returns False if there are no results for the store."""
    points = load_points(store)
    if not points:
        return False
    fig, axes = plt.subplots(2, 2, figsize=(12, 9))
    for ax, (title, complexity, ops, workloads) in zip(axes.flat, PANELS[store]):
        draw_panel(ax, title, complexity, ops, workloads, points)
    fig.suptitle(SUPTITLE[store])
    fig.tight_layout()
    dest = OUT_DIR / f"{store}.png"
    fig.savefig(dest, dpi=150)
    plt.close(fig)
    print(f"wrote {dest}")

    print(f"{'op':21} {'workload':8} {'n':>6}  median_us")
    for (op, workload), series in sorted(points.items()):
        for n, ns in series:
            print(f"{op:21} {workload:8} {n:6}  {ns / 1_000:.3f}")
    print()
    return True


def main() -> None:
    if not CRITERION.is_dir():
        sys.exit(f"no Criterion output at {CRITERION}; run `cargo bench` first")
    OUT_DIR.mkdir(exist_ok=True)
    wrote = [plot_store(store) for store in PANELS]
    for store, ok in zip(PANELS, wrote):
        if not ok:
            print(f"skipped {store}: no results under {CRITERION}; run `cargo bench -- /{store}/` first")
    wrote.append(plot_comparison())
    if not wrote[-1]:
        print(f"skipped compare.png: no results for {COMPARISON_WORKLOADS} under {CRITERION}")
    if not any(wrote):
        sys.exit("nothing to plot")


if __name__ == "__main__":
    main()
