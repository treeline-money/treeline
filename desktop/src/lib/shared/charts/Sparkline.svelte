<script lang="ts">
  /**
   * Sparkline - A compact inline chart for showing trends
   *
   * Usage:
   *   <Sparkline data={[100, 120, 115, 130, 125, 140]} />
   */

  interface Props {
    data: number[];
    width?: number;
    height?: number;
    strokeWidth?: number;
    color?: string;
    positiveColor?: string;
    negativeColor?: string;
  }

  let {
    data,
    width = 80,
    height = 16,
    strokeWidth = 1.5,
    color,
    positiveColor = "var(--accent-success, #22c55e)",
    negativeColor = "var(--accent-danger, #ef4444)",
  }: Props = $props();

  // Compute path and color based on data
  let pathD = $derived.by(() => {
    if (data.length < 2) return "";

    const min = Math.min(...data);
    const max = Math.max(...data);
    const range = max - min || 1;

    // Add padding so line doesn't touch edges
    const padding = strokeWidth;
    const chartWidth = width - padding * 2;
    const chartHeight = height - padding * 2;

    const points = data.map((value, i) => {
      const x = padding + (i / (data.length - 1)) * chartWidth;
      const y = padding + chartHeight - ((value - min) / range) * chartHeight;
      return `${x},${y}`;
    });

    return `M ${points.join(" L ")}`;
  });

  // Determine color based on trend (first vs last value)
  let strokeColor = $derived.by(() => {
    if (color) return color;
    if (data.length < 2) return positiveColor;
    const first = data[0];
    const last = data[data.length - 1];
    return last >= first ? positiveColor : negativeColor;
  });
</script>

<svg
  {width}
  {height}
  viewBox="0 0 {width} {height}"
  class="sparkline"
  aria-hidden="true"
>
  {#if pathD}
    <path
      d={pathD}
      fill="none"
      stroke={strokeColor}
      stroke-width={strokeWidth}
      stroke-linecap="round"
      stroke-linejoin="round"
    />
  {/if}
</svg>

<style>
  .sparkline {
    display: inline-block;
    vertical-align: middle;
  }
</style>
