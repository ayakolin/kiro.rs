import { cn } from '@/lib/utils'

/**
 * 凭据标识：`#id` + 邮箱。
 *
 * 为什么把 id 摆到明面上：日志页的故障转移链路、每跳明细、批量操作的结果提示，
 * 全都以 `#id` 称呼凭据（邮箱太长，塞不进那些紧凑位置）。但凭据行此前只显示邮箱，
 * 于是"日志里那个 #9 是谁"得靠数据库或猜。把 id 常驻在邮箱左侧，这条线就接上了。
 *
 * 视觉上 id 明确从属：等宽、弱化、字号更小，邮箱仍是主读物。用等宽是因为列表视图里
 * id 会纵向堆叠，非等宽数字会参差不齐。
 */
export function CredentialLabel({
  id,
  email,
  className,
  idClassName,
  showId = true,
}: {
  id: number
  email?: string | null
  className?: string
  idClassName?: string
  showId?: boolean
}) {
  return (
    <span className={cn('inline-flex min-w-0 items-baseline gap-1.5', className)}>
      {showId && (
        <span
          className={cn(
            'console-num shrink-0 text-[0.85em] font-normal text-muted-foreground/70',
            idClassName,
          )}
          title={`凭据 ID ${id} —— 日志与链路里以 #${id} 指代`}
        >
          #{id}
        </span>
      )}
      <span className="min-w-0 truncate">
        {email || <span className="text-muted-foreground">未设置邮箱</span>}
      </span>
    </span>
  )
}
