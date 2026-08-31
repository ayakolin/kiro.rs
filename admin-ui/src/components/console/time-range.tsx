import { useState } from 'react'
import { Clock } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import {
  DropdownMenu,
  DropdownMenuTrigger,
  DropdownMenuContent,
  DropdownMenuLabel,
} from '@/components/ui/dropdown-menu'
import { cn } from '@/lib/utils'

/**
 * 时间范围选择器。
 *
 * 日志页此前缺的就是这个维度 —— 排查现场的第一句话通常是"刚才那几分钟发生了
 * 什么"，而不是"给我看最近 50 条"。预设覆盖了实际会用到的档：出事当下看 15 分钟，
 * 复盘看 1 小时 / 24 小时，追趋势看 7 天。
 *
 * 相对时间（"最近 N 分钟"）而非绝对区间是默认档，因为它在自动刷新下语义稳定：
 * 30 秒后重新拉取时窗口跟着滑动，看到的仍然是"最近 15 分钟"。
 */
export interface TimeRange {
  /** 相对窗口分钟数；null = 不限时间 */
  minutes: number | null
}

export const TIME_PRESETS: { label: string; minutes: number | null }[] = [
  { label: '最近 15 分钟', minutes: 15 },
  { label: '最近 1 小时', minutes: 60 },
  { label: '最近 24 小时', minutes: 60 * 24 },
  { label: '最近 7 天', minutes: 60 * 24 * 7 },
  { label: '不限时间', minutes: null },
]

const MAX_CUSTOM_MINUTES = 60 * 24 * 90

export function rangeLabel(range: TimeRange): string {
  const hit = TIME_PRESETS.find((p) => p.minutes === range.minutes)
  if (hit) return hit.label
  const m = range.minutes!
  if (m % (60 * 24) === 0) return `最近 ${m / (60 * 24)} 天`
  if (m % 60 === 0) return `最近 ${m / 60} 小时`
  return `最近 ${m} 分钟`
}

/** 把相对窗口换算成后端要的起始毫秒时间戳；null = 不传 */
export function rangeToStartMs(range: TimeRange, now = Date.now()): number | null {
  if (range.minutes == null) return null
  return now - range.minutes * 60_000
}

export function TimeRangePicker({
  value,
  onChange,
  disabled,
}: {
  value: TimeRange
  onChange: (next: TimeRange) => void
  disabled?: boolean
}) {
  const [open, setOpen] = useState(false)
  const [custom, setCustom] = useState('')
  const [invalid, setInvalid] = useState(false)

  const submitCustom = (e: React.FormEvent) => {
    e.preventDefault()
    const n = parseInt(custom, 10)
    if (!Number.isFinite(n) || n < 1 || n > MAX_CUSTOM_MINUTES) {
      setInvalid(true)
      window.setTimeout(() => setInvalid(false), 1200)
      return
    }
    onChange({ minutes: n })
    setCustom('')
    setOpen(false)
  }

  return (
    <DropdownMenu open={open} onOpenChange={setOpen}>
      <DropdownMenuTrigger asChild>
        <Button
          size="sm"
          variant="outline"
          disabled={disabled}
          title="按时间范围筛选"
        >
          <Clock className="h-3.5 w-3.5" />
          {rangeLabel(value)}
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-56">
        <DropdownMenuLabel>时间范围</DropdownMenuLabel>
        <div className="grid gap-1 px-2 pb-2">
          {TIME_PRESETS.map((p) => (
            <Button
              key={p.label}
              size="sm"
              variant={value.minutes === p.minutes ? 'default' : 'ghost'}
              className="h-7 justify-start px-2 text-xs"
              onClick={() => {
                onChange({ minutes: p.minutes })
                setOpen(false)
              }}
            >
              {p.label}
            </Button>
          ))}
        </div>
        <DropdownMenuLabel className="pt-0">自定义</DropdownMenuLabel>
        <form onSubmit={submitCustom} className="flex items-center gap-1.5 px-2 pb-2">
          <Input
            type="number"
            min={1}
            max={MAX_CUSTOM_MINUTES}
            placeholder="分钟"
            value={custom}
            onChange={(e) => setCustom(e.target.value)}
            className={cn(
              'console-num h-7 text-xs',
              invalid && 'border-destructive',
            )}
          />
          <Button
            type="submit"
            size="sm"
            variant="outline"
            className="h-7 text-xs"
            disabled={!custom.trim()}
          >
            应用
          </Button>
        </form>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
