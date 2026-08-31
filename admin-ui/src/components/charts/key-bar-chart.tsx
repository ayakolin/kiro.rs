import { memo, useMemo } from 'react'
import { BarChart, Bar, XAxis, YAxis, CartesianGrid, Tooltip, ResponsiveContainer, Legend } from 'recharts'
import type { KeyDistribution } from '@/types/api'
import { tooltipContentStyle, tooltipCursorStyle, tooltipItemStyle, tooltipLabelStyle } from './tooltip-style'
import { formatNumber } from '@/lib/utils'

interface Props {
  data: KeyDistribution[]
}

interface ChartDatum {
  calls: number
  errors: number
  fullLabel: string
  inputTokens: number
  label: string
  outputTokens: number
}

function KeyBarChartImpl({ data }: Props) {
  const formatted = useMemo(() => buildChartData(data), [data])

  if (data.length === 0) {
    return <EmptyKeyChart />
  }

  return <KeyChartContent data={formatted} />
}

function buildChartData(data: KeyDistribution[]): ChartDatum[] {
  return data.slice(0, 12).map((d) => {
    const fullLabel = d.name || `#${d.keyId}`
    return {
      calls: d.calls,
      errors: d.errors,
      fullLabel,
      inputTokens: d.inputTokens,
      label: truncateLabel(fullLabel),
      outputTokens: d.outputTokens,
    }
  })
}

function EmptyKeyChart() {
  return (
    <div className="flex h-[180px] items-center justify-center text-sm text-muted-foreground sm:h-[260px]">
      暂无数据
    </div>
  )
}

function KeyChartContent({ data }: { data: ChartDatum[] }) {
  return (
    <div className="h-[280px] sm:h-[340px]">
      <ResponsiveContainer width="100%" height="100%">
        <BarChart data={data} margin={{ top: 8, right: 8, left: -10, bottom: 52 }}>
          {keyChartAxes()}
          {keyChartTooltip()}
          <Legend verticalAlign="top" align="right" height={28} wrapperStyle={{ fontSize: 12 }} />
          {keyChartBars()}
        </BarChart>
      </ResponsiveContainer>
    </div>
  )
}

function keyChartAxes() {
  return [
    <CartesianGrid key="grid" strokeDasharray="3 3" className="stroke-border/50" />,
    <XAxis
      key="x"
      dataKey="label"
      tick={{ fontSize: 10 }}
      angle={-30}
      textAnchor="end"
      interval={0}
      height={64}
    />,
    <YAxis key="y" tick={{ fontSize: 11 }} tickFormatter={(v: number) => formatNumber(v)} width={42} />,
  ]
}

function keyChartTooltip() {
  return (
    <Tooltip
      contentStyle={tooltipContentStyle}
      labelStyle={tooltipLabelStyle}
      itemStyle={tooltipItemStyle}
      cursor={tooltipCursorStyle}
      formatter={(value: number) => formatNumber(value)}
      labelFormatter={formatTooltipLabel}
    />
  )
}

function formatTooltipLabel(label: string, payload?: ReadonlyArray<{ payload?: ChartDatum }>) {
  return payload?.[0]?.payload?.fullLabel ?? label
}

function keyChartBars() {
  return [
    <Bar key="input" dataKey="inputTokens" name="输入" stackId="a" fill="#6366f1" isAnimationActive={false} />,
    <Bar key="output" dataKey="outputTokens" name="输出" stackId="a" fill="#f59e0b" isAnimationActive={false} />,
  ]
}

export const KeyBarChart = memo(KeyBarChartImpl)

/** 仅用于 X 轴展示：整体最长 18 字符，超出截断 */
function truncateLabel(label: string): string {
  if (label.length <= 18) return label
  return label.slice(0, 17) + '…'
}
