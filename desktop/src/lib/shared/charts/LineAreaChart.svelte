<script lang="ts">
  /**
   * LineAreaChart - A line chart with optional filled area and hover tooltips
   */

  import type { DataPoint } from "./types";

  interface Props {
    data: DataPoint[];
    height?: number;
    showArea?: boolean;
    showLine?: boolean;
    showLabels?: boolean;
    showZeroLine?: boolean;
    positiveColor?: string;
    negativeColor?: string;
    lineWidth?: number;
    formatValue?: (value: number) => string;
    invertTrend?: boolean; // For liabilities: decreasing is good
  }

  let {
    data,
    height = 120,
    showArea = true,
    showLine = true,
    showLabels = true,
    showZeroLine = true,
    positiveColor = "var(--accent-success, #22c55e)",
    negativeColor = "var(--accent-danger, #ef4444)",
    lineWidth = 1.5,
    formatValue = (v: number) => v.toLocaleString(),
    invertTrend = false,
  }: Props = $props();

  // Hover state
  let hoverIndex = $state<number | null>(null);
  let containerEl: HTMLDivElement | null = $state(null);

  // Layout constants
  const VIEWBOX_WIDTH = 1000;
  const VIEWBOX_HEIGHT = height;
  const PADDING_TOP = 10;
  const PADDING_BOTTOM = showLabels ? 4 : 8;
  const PADDING_LEFT = 0;
  const PADDING_RIGHT = 0;

  // Compute chart dimensions and scales
  let chartData = $derived.by(() => {
    if (data.length < 2) return null;

    const values = data.map((d) => d.value);
    const minValue = Math.min(...values);
    const maxValue = Math.max(...values);

    const hasNegative = minValue < 0;
    const hasPositive = maxValue > 0;
    const crossesZero = hasNegative && hasPositive;

    let rangeMin = minValue;
    let rangeMax = maxValue;

    if (showZeroLine) {
      if (hasNegative && !hasPositive) {
        rangeMax = Math.max(0, maxValue);
      } else if (hasPositive && !hasNegative) {
        rangeMin = minValue - (maxValue - minValue) * 0.1;
      }
    }

    const valueRange = rangeMax - rangeMin || 1;
    rangeMin -= valueRange * 0.05;
    rangeMax += valueRange * 0.05;

    const range = rangeMax - rangeMin;
    const chartHeight = VIEWBOX_HEIGHT - PADDING_TOP - PADDING_BOTTOM;
    const chartWidth = VIEWBOX_WIDTH - PADDING_LEFT - PADDING_RIGHT;

    const valueToY = (value: number) => {
      return PADDING_TOP + chartHeight - ((value - rangeMin) / range) * chartHeight;
    };

    const zeroY = valueToY(0);

    return {
      values,
      minValue,
      maxValue,
      rangeMin,
      rangeMax,
      range,
      chartHeight,
      chartWidth,
      hasNegative,
      hasPositive,
      crossesZero,
      zeroY,
      valueToY,
    };
  });

  // Generate SVG path for the line
  let linePath = $derived.by(() => {
    if (!chartData || data.length < 2) return "";

    const stepX = chartData.chartWidth / (data.length - 1);

    const points = data.map((d, i) => {
      const x = PADDING_LEFT + i * stepX;
      const y = chartData!.valueToY(d.value);
      return `${x.toFixed(2)},${y.toFixed(2)}`;
    });

    return `M ${points.join(" L ")}`;
  });

  // Generate SVG path for the area fill
  let areaPath = $derived.by(() => {
    if (!chartData || data.length < 2 || !showArea) return "";

    const stepX = chartData.chartWidth / (data.length - 1);
    const bottomY = VIEWBOX_HEIGHT - PADDING_BOTTOM;

    const points = data.map((d, i) => {
      const x = PADDING_LEFT + i * stepX;
      const y = chartData!.valueToY(d.value);
      return { x, y };
    });

    const startX = points[0].x;
    const endX = points[points.length - 1].x;

    const linePoints = points.map((p) => `${p.x.toFixed(2)},${p.y.toFixed(2)}`).join(" L ");

    return `M ${startX.toFixed(2)},${bottomY} L ${linePoints} L ${endX.toFixed(2)},${bottomY} Z`;
  });

  // All point positions for hover detection
  let allPoints = $derived.by(() => {
    if (!chartData || data.length < 2) return [];

    const stepX = chartData.chartWidth / (data.length - 1);

    return data.map((d, i) => ({
      x: PADDING_LEFT + i * stepX,
      y: chartData!.valueToY(d.value),
      xPercent: ((PADDING_LEFT + i * stepX) / VIEWBOX_WIDTH) * 100,
      label: d.label,
      value: d.value,
      index: i,
    }));
  });

  // Hovered point
  let hoveredPoint = $derived(hoverIndex !== null ? allPoints[hoverIndex] : null);

  // Generate smart label positions - avoid overlaps
  let labelPositions = $derived.by(() => {
    if (!chartData || data.length < 2 || !showLabels) return [];

    const stepX = chartData.chartWidth / (data.length - 1);
    const labels: { x: number; label: string; index: number }[] = [];

    // Always show first label
    if (data[0].label) {
      labels.push({ x: PADDING_LEFT, label: data[0].label, index: 0 });
    }

    // Show intermediate labels, but check they don't overlap
    const minLabelSpacing = VIEWBOX_WIDTH * 0.12; // Min 12% spacing between labels

    for (let i = 1; i < data.length - 1; i++) {
      if (!data[i].label) continue;

      const x = PADDING_LEFT + i * stepX;
      const lastLabel = labels[labels.length - 1];

      if (lastLabel && x - lastLabel.x < minLabelSpacing) continue;

      // Also check distance to end
      const endX = PADDING_LEFT + (data.length - 1) * stepX;
      if (endX - x < minLabelSpacing) continue;

      labels.push({ x, label: data[i].label, index: i });
    }

    // Always show last label
    if (data[data.length - 1].label) {
      labels.push({
        x: PADDING_LEFT + (data.length - 1) * stepX,
        label: data[data.length - 1].label,
        index: data.length - 1,
      });
    }

    return labels;
  });

  // Determine primary color based on overall trend
  // For invertTrend (liabilities), decreasing is good (green)
  let primaryColor = $derived.by(() => {
    if (data.length < 2) return positiveColor;
    const first = data[0].value;
    const last = data[data.length - 1].value;
    const isIncreasing = last >= first;
    // For liabilities, invert: decreasing = good
    const isPositiveTrend = invertTrend ? !isIncreasing : isIncreasing;
    return isPositiveTrend ? positiveColor : negativeColor;
  });

  // Area fill uses same color as line (based on trend direction)
  let areaFillColor = $derived(primaryColor);

  // Handle mouse move for hover
  function handleMouseMove(e: MouseEvent) {
    if (!containerEl || !chartData || data.length < 2) {
      hoverIndex = null;
      return;
    }

    const rect = containerEl.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const xPercent = (x / rect.width) * 100;

    // Find closest point
    let closestIndex = 0;
    let closestDist = Infinity;

    for (let i = 0; i < allPoints.length; i++) {
      const dist = Math.abs(allPoints[i].xPercent - xPercent);
      if (dist < closestDist) {
        closestDist = dist;
        closestIndex = i;
      }
    }

    hoverIndex = closestIndex;
  }

  function handleMouseLeave() {
    hoverIndex = null;
  }
</script>

<div
  class="line-area-chart"
  bind:this={containerEl}
  onmousemove={handleMouseMove}
  onmouseleave={handleMouseLeave}
  role="img"
  aria-label="Line chart"
>
  <svg
    width="100%"
    height={height}
    viewBox="0 0 {VIEWBOX_WIDTH} {VIEWBOX_HEIGHT}"
    preserveAspectRatio="none"
    class="chart-svg"
  >
    {#if chartData && data.length >= 2}
      <!-- Zero line -->
      {#if showZeroLine && (chartData.crossesZero || chartData.hasNegative)}
        <line
          x1={PADDING_LEFT}
          y1={chartData.zeroY}
          x2={VIEWBOX_WIDTH - PADDING_RIGHT}
          y2={chartData.zeroY}
          class="zero-line"
        />
      {/if}

      <!-- Area fill -->
      {#if showArea && areaPath}
        <path d={areaPath} class="area-fill" style="fill: {areaFillColor}" />
      {/if}

      <!-- Line -->
      {#if showLine && linePath}
        <path
          d={linePath}
          class="chart-line"
          style="stroke: {primaryColor}"
          stroke-width={lineWidth}
        />
      {/if}

      <!-- Hover vertical line -->
      {#if hoveredPoint}
        <line
          x1={hoveredPoint.x}
          y1={PADDING_TOP}
          x2={hoveredPoint.x}
          y2={VIEWBOX_HEIGHT - PADDING_BOTTOM}
          class="hover-line"
        />
        <circle
          cx={hoveredPoint.x}
          cy={hoveredPoint.y}
          r="6"
          class="hover-point"
          style="fill: {primaryColor}"
        />
      {/if}
    {/if}
  </svg>

  <!-- Tooltip -->
  {#if hoveredPoint}
    <div
      class="tooltip"
      style="left: {hoveredPoint.xPercent}%"
      class:flip-left={hoveredPoint.xPercent > 75}
      class:flip-right={hoveredPoint.xPercent < 25}
    >
      <div class="tooltip-value">{formatValue(hoveredPoint.value)}</div>
      {#if hoveredPoint.label}
        <div class="tooltip-label">{hoveredPoint.label}</div>
      {/if}
    </div>
  {/if}

  <!-- X-axis labels -->
  {#if showLabels && labelPositions.length > 0}
    <div class="x-labels">
      {#each labelPositions as lp}
        <span
          class="x-label"
          class:first={lp.index === 0}
          class:last={lp.index === data.length - 1}
          style="left: {(lp.x / VIEWBOX_WIDTH) * 100}%"
        >
          {lp.label}
        </span>
      {/each}
    </div>
  {/if}
</div>

<style>
  .line-area-chart {
    position: relative;
    width: 100%;
    cursor: crosshair;
  }

  .chart-svg {
    display: block;
    overflow: visible;
  }

  .zero-line {
    stroke: var(--border-primary, #374151);
    stroke-width: 1;
    stroke-dasharray: 4, 2;
    opacity: 0.4;
  }

  .area-fill {
    opacity: 0.15;
  }

  .chart-line {
    fill: none;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .hover-line {
    stroke: var(--text-muted, #6b7280);
    stroke-width: 1;
    stroke-dasharray: 2, 2;
    opacity: 0.5;
  }

  .hover-point {
    stroke: var(--bg-primary, #111827);
    stroke-width: 2;
  }

  .tooltip {
    position: absolute;
    top: 0;
    transform: translateX(-50%);
    background: var(--bg-tertiary, #374151);
    border: 1px solid var(--border-primary, #4b5563);
    border-radius: 4px;
    padding: 4px 8px;
    pointer-events: none;
    z-index: 10;
    white-space: nowrap;
  }

  .tooltip.flip-left {
    transform: translateX(-90%);
  }

  .tooltip.flip-right {
    transform: translateX(-10%);
  }

  .tooltip-value {
    font-size: 12px;
    font-weight: 600;
    font-family: var(--font-mono);
    color: var(--text-primary, #f3f4f6);
  }

  .tooltip-label {
    font-size: 10px;
    color: var(--text-muted, #9ca3af);
  }

  .x-labels {
    position: relative;
    height: 16px;
    margin-top: 4px;
  }

  .x-label {
    position: absolute;
    transform: translateX(-50%);
    font-size: 10px;
    color: var(--text-muted, #6b7280);
    white-space: nowrap;
  }

  .x-label.first {
    transform: translateX(0);
  }

  .x-label.last {
    transform: translateX(-100%);
    font-weight: 500;
    color: var(--text-secondary, #9ca3af);
  }
</style>
