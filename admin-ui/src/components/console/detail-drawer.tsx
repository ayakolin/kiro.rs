import * as DialogPrimitive from '@radix-ui/react-dialog'
import { X } from 'lucide-react'
import { cn } from '@/lib/utils'

/**
 * 右侧详情抽屉。
 *
 * 替掉日志页的「展开行」：展开会把表格行高从 34px 顶到几百 px，下方所有行
 * 位置突变，滚动位置随之跳一下 —— 想对比相邻两条记录时尤其难受。抽屉把详情
 * 移到侧面，表格布局始终稳定，左侧列表和右侧详情能同时看。
 *
 * 基于 Radix Dialog：焦点陷阱、Esc 关闭、滚动锁都是现成的，只把定位从居中
 * 改成贴右侧全高。
 */
export function DetailDrawer({
  open,
  onOpenChange,
  title,
  subtitle,
  children,
  footer,
  width = 'max-w-xl',
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  title: React.ReactNode
  subtitle?: React.ReactNode
  children: React.ReactNode
  footer?: React.ReactNode
  /** tailwind 宽度类 */
  width?: string
}) {
  return (
    <DialogPrimitive.Root open={open} onOpenChange={onOpenChange}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay className="fixed inset-0 z-50 bg-black/25 backdrop-blur-sm data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0" />
        <DialogPrimitive.Content
          className={cn(
            'console-drawer console-scope fixed right-0 top-0 z-50 flex h-full w-[calc(100%-1.5rem)] flex-col border-l border-border/60 bg-card/95 shadow-apple-lg backdrop-blur-2xl backdrop-saturate-150',
            width,
          )}
        >
          <div className="flex items-start gap-3 border-b border-border/60 px-4 py-3">
            <div className="min-w-0 flex-1">
              <DialogPrimitive.Title className="truncate text-[15px] font-semibold tracking-tight">
                {title}
              </DialogPrimitive.Title>
              {subtitle && (
                <DialogPrimitive.Description className="mt-0.5 truncate text-xs text-muted-foreground">
                  {subtitle}
                </DialogPrimitive.Description>
              )}
            </div>
            <DialogPrimitive.Close
              className="inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-secondary/80 text-muted-foreground transition-colors hover:bg-secondary hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              title="关闭（Esc）"
            >
              <X className="h-3.5 w-3.5" />
              <span className="sr-only">关闭</span>
            </DialogPrimitive.Close>
          </div>

          <div className="min-h-0 flex-1 overflow-y-auto px-4 py-3">{children}</div>

          {footer && (
            <div className="flex items-center justify-end gap-2 border-t border-border/60 px-4 py-3">
              {footer}
            </div>
          )}
        </DialogPrimitive.Content>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  )
}

/** 抽屉内的键值明细行 —— 详情面板的基本排版单元 */
export function DrawerField({
  label,
  children,
  mono = false,
}: {
  label: string
  children: React.ReactNode
  mono?: boolean
}) {
  return (
    <div className="flex items-baseline justify-between gap-4 py-1.5 text-[13px]">
      <span className="shrink-0 text-muted-foreground">{label}</span>
      <span className={cn('min-w-0 text-right', mono && 'console-num')}>
        {children}
      </span>
    </div>
  )
}

/** 抽屉内的分区标题 */
export function DrawerSection({
  title,
  children,
  trailing,
}: {
  title: string
  children: React.ReactNode
  trailing?: React.ReactNode
}) {
  return (
    <section className="py-2">
      <div className="mb-1.5 flex items-center gap-2">
        <h4 className="text-[11px] font-medium uppercase tracking-[0.06em] text-muted-foreground">
          {title}
        </h4>
        <span className="h-px flex-1 bg-border/60" />
        {trailing}
      </div>
      {children}
    </section>
  )
}
