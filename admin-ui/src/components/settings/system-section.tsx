import { useEffect, useState } from 'react'
import { toast } from 'sonner'
import { UploadCloud, Eye, EyeOff } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import {
  SettingGroup,
  SettingRow,
  SettingSwitch,
  useFieldSaver,
} from '@/components/console/setting-row'
import { useUpdateConfig, useSetUpdateConfig } from '@/hooks/use-credentials'
import { useUpdateCheck } from '@/hooks/use-update-check'
import { ImageUpdateDialog } from '@/components/image-update-dialog'
import { reportSaveError } from '@/components/settings/report-error'

/**
 * 系统分区：镜像在线更新的**配置**。
 *
 * 与更新弹窗的分工：这里管"什么时候自动更新、用哪个 token 查版本"，弹窗管
 * "现在检查 / 拉取 / 应用 / 回退"这套动作流程。两边读同一个 ['update-config']
 * query key，改任何一处另一处立即跟上。
 *
 * 时间格式约定 HH:MM 24 小时制，按服务器本地时区执行。
 */
const TIME_PATTERN = /^([01]\d|2[0-3]):([0-5]\d)$/

export function SystemSection() {
  const { data, isLoading } = useUpdateConfig()
  const { mutate } = useSetUpdateConfig()
  const saver = useFieldSaver(mutate, reportSaveError)
  const { data: check } = useUpdateCheck()
  const [dialogOpen, setDialogOpen] = useState(false)

  const autoApply = data?.autoApply ?? false

  return (
    <>
      <div className="space-y-6">
        <SettingGroup
          title="版本"
          description="镜像更新的执行入口在更新面板里；这里只配自动化策略"
        >
          <SettingRow
            label="当前版本"
            hint={
              check?.hasUpdate
                ? `有新版本 v${check.latestVersion} 可用`
                : '已是最新版本'
            }
          >
            <div className="flex items-center gap-2">
              <span className="console-num text-[13px]">
                v{check?.currentVersion ?? '—'}
              </span>
              <Button size="sm" variant="outline" onClick={() => setDialogOpen(true)}>
                <UploadCloud className="h-3.5 w-3.5" />
                更新面板
              </Button>
            </div>
          </SettingRow>
        </SettingGroup>

        <SettingGroup title="无人值守更新">
          <SettingSwitch
            label="自动更新"
            hint={
              autoApply
                ? '每天到点检查新版本，有则自动拉取并重启容器'
                : '仅提示新版本，更新需要手动触发'
            }
            checked={autoApply}
            onChange={(next) => saver.save('autoApply', { autoApply: next })}
            pending={saver.isSaving('autoApply')}
            saved={saver.isSaved('autoApply')}
            disabled={isLoading}
          />
          <AutoApplyTimeRow
            value={data?.autoApplyTime ?? '03:00'}
            onCommit={(t) => saver.save('autoApplyTime', { autoApplyTime: t })}
            pending={saver.isSaving('autoApplyTime')}
            saved={saver.isSaved('autoApplyTime')}
            disabled={isLoading || !autoApply}
          />
        </SettingGroup>

        <SettingGroup
          title="GitHub Token"
          description="用于查询版本，避免匿名调用触发 GitHub API 限流（每小时 60 次）"
        >
          <GithubTokenRow
            tokenSet={data?.githubTokenSet ?? false}
            onCommit={(token) => saver.save('githubToken', { githubToken: token })}
            pending={saver.isSaving('githubToken')}
            saved={saver.isSaved('githubToken')}
            disabled={isLoading}
          />
        </SettingGroup>
      </div>

      <ImageUpdateDialog open={dialogOpen} onOpenChange={setDialogOpen} />
    </>
  )
}

function AutoApplyTimeRow({
  value,
  onCommit,
  pending,
  saved,
  disabled,
}: {
  value: string
  onCommit: (next: string) => void
  pending?: boolean
  saved?: boolean
  disabled?: boolean
}) {
  const [draft, setDraft] = useState(value)

  useEffect(() => {
    setDraft(value)
  }, [value])

  const commit = () => {
    const t = draft.trim()
    if (!TIME_PATTERN.test(t)) {
      toast.error('时间格式为 HH:MM（24 小时制）')
      setDraft(value)
      return
    }
    if (t !== value) onCommit(t)
  }

  return (
    <SettingRow
      label="触发时间"
      hint="按服务器本地时区。建议选业务低谷，更新会重启容器造成短暂中断"
      pending={pending}
      saved={saved}
      disabled={disabled}
    >
      <Input
        type="time"
        value={draft}
        disabled={disabled || pending}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === 'Enter') e.currentTarget.blur()
          if (e.key === 'Escape') {
            setDraft(value)
            e.currentTarget.blur()
          }
        }}
        className="console-num h-8 w-28 text-[13px]"
      />
    </SettingRow>
  )
}

/**
 * Token 行：写入型密文字段，只回布尔不回明文，所以走显式「保存」。
 * 这是即时保存范式的第二个例外，与代理地址同理 —— 密文没有"当前值"可回显，
 * 失焦提交会把用户输了一半的 token 存进去。
 */
function GithubTokenRow({
  tokenSet,
  onCommit,
  pending,
  saved,
  disabled,
}: {
  tokenSet: boolean
  onCommit: (token: string) => void
  pending?: boolean
  saved?: boolean
  disabled?: boolean
}) {
  const [draft, setDraft] = useState('')
  const [plain, setPlain] = useState(false)

  return (
    <SettingRow
      label="Personal Access Token"
      hint={
        tokenSet
          ? '已配置。重新输入可替换，留空点「清除」可移除'
          : '未配置，当前以匿名身份查询版本'
      }
      pending={pending}
      saved={saved}
    >
      <div className="flex flex-wrap items-center gap-1.5">
        <div className="relative">
          <Input
            type={plain ? 'text' : 'password'}
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            placeholder={tokenSet ? '输入新 token 以替换' : 'ghp_…'}
            disabled={disabled || pending}
            spellCheck={false}
            autoComplete="off"
            className="console-num h-8 w-[min(16rem,50vw)] pr-9 text-[12.5px]"
          />
          <Button
            type="button"
            size="icon"
            variant="ghost"
            onClick={() => setPlain((v) => !v)}
            title={plain ? '隐藏' : '显示'}
            className="absolute right-0.5 top-0.5 h-7 w-7"
          >
            {plain ? <EyeOff className="h-3.5 w-3.5" /> : <Eye className="h-3.5 w-3.5" />}
          </Button>
        </div>
        <Button
          size="sm"
          variant="outline"
          disabled={disabled || pending || !draft.trim()}
          onClick={() => {
            onCommit(draft.trim())
            setDraft('')
            setPlain(false)
          }}
        >
          保存
        </Button>
        {tokenSet && (
          <Button
            size="sm"
            variant="ghost"
            disabled={pending}
            onClick={() => {
              onCommit('')
              setDraft('')
            }}
            title="移除已保存的 token，恢复匿名查询"
          >
            清除
          </Button>
        )}
      </div>
    </SettingRow>
  )
}
