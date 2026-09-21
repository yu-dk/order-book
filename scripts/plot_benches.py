#!/usr/bin/env python3
"""Plot Criterion results from target/criterion/<op>_<workload>/<store>/<n>/new/estimates.json.

Writes plots/btree.png and plots/bucket_map.png: one subplot per operation,
titled with its time complexity, each with the three workloads (uniform,
normal, hot). The median time in microseconds is plotted against the book
size n. O(1), O(n) and O(log m) subplots use linear axes, so the distance
between sizes is proportional to n. O(log n) subplots plot log2(n) on the x
axis instead, so a true O(log n) cost is a straight line.

bucket_map writes cost O(log m), where m is the number of orders at one price,
and m <= n. `_hot` has m = n/8, so its curve should bend like log n on the
linear axis, while `_uniform` and `_normal` have m of about 1 to 6, so their
curves should be nearly flat.
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

SIZES = (1_000, 4_000, 16_000, 64_000)  # SIZES in benches/order_book.rs; other sizes are leftovers of older runs
WORKLOADS = ("uniform", "normal", "hot")
GROUP = re.compile(r"^(insert|update_quantity_only|remove|bids|asks)_(uniform|normal|hot)$")

WORKLOAD_COLOR = {"uniform": "#1b6ca8", "normal": "#d95f02", "hot": "#2a9d55"}
OP_STYLE = {"bids": ("-", "o"), "asks": ("--", "s")}  # ops that share a subplot; the rest use ("-", "o")

# Subplots per store: (title, complexity, operations drawn in it)
PANELS = {
    "btree": (
        ("update_quantity_only: O(1)", "1", ("update_quantity_only",)),
        ("insert: O(log n)", "log n", ("insert",)),
        ("remove: O(log n)", "log n", ("remove",)),
        ("bids/asks: O(n)", "n", ("bids", "asks")),
    ),
    "bucket_map": (
        ("update_quantity_only: O(log m)", "log m", ("update_quantity_only",)),
        ("insert: O(log m)", "log m", ("insert",)),
        ("remove: O(log m)", "log m", ("remove",)),
        ("bids/asks: O(n)", "n", ("bids", "asks")),
    ),
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


def draw_panel(ax, title: str, complexity: str, ops: tuple[str, ...], points: dict[tuple[str, str], list[tuple[int, float]]]) -> None:
    log_x = complexity == "log n"
    for op in ops:
        style, marker = OP_STYLE.get(op, ("-", "o"))
        for workload in WORKLOADS:
            pts = points.get((op, workload))
            if not pts:
                continue
            label = workload if len(ops) == 1 else f"{op} / {workload}"
            xs = [math.log2(n) if log_x else n for n, _ in pts]
            ax.plot(xs, [t / 1_000.0 for _, t in pts], style, marker=marker, color=WORKLOAD_COLOR[workload], label=label)

    if log_x:
        ax.set_xticks([math.log2(n) for n in SIZES], [f"{math.log2(n):.1f}\n(n = {n:,})" for n in SIZES])
        ax.set_xlabel("log2(book size n)")
    else:
        ax.set_xticks(SIZES, [f"{n:,}" for n in SIZES])
        ax.tick_params(axis="x", labelrotation=45)  # 1,000 and 4,000 are close on a linear axis
        ax.set_xlabel("book size n")
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
    for ax, (title, complexity, ops) in zip(axes.flat, PANELS[store]):
        draw_panel(ax, title, complexity, ops, points)
    fig.suptitle(store)
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
    if not any(wrote):
        sys.exit("nothing to plot")


if __name__ == "__main__":
    main()
