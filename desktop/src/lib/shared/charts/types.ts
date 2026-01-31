/**
 * Shared Chart Types
 */

export interface DataPoint {
  label: string;
  value: number;
}

export interface LineAreaChartProps {
  data: DataPoint[];
  height?: number;
  showArea?: boolean;
  showLine?: boolean;
  showPoints?: boolean;
  showLabels?: boolean;
  showValues?: boolean;
  showZeroLine?: boolean;
  positiveColor?: string;
  negativeColor?: string;
  lineWidth?: number;
  pointRadius?: number;
  formatValue?: (value: number) => string;
}

export interface SparklineProps {
  data: number[];
  width?: number;
  height?: number;
  strokeWidth?: number;
  color?: string;
  positiveColor?: string;
  negativeColor?: string;
}

export interface BarChartProps {
  data: DataPoint[];
  maxValue?: number;
  showLabels?: boolean;
  showValues?: boolean;
  formatValue?: (value: number) => string;
}
