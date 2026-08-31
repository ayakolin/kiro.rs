import { useState } from 'react'
import { toast } from 'sonner'
import { Eye, EyeOff, Copy, Wand2 } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { SettingGroup, SettingRow } from '@/components/console/setting-row'
import { storage } from '@/lib/storage'
import { updateAdminKey } from '@/api/credentials'
import { extractErrorMessage, generateApiKey } from '@/lib/utils'
import { useConfirm } from '@/components/ui/confirm-dialog'

/**
 * 安全分区：管理面板的登录密钥。
 *
 * 这里刻意**不用**即时保存。轮换登录密钥不是调参数，是一次性的凭据替换：旧密钥
 * 立即失效，正在用它调用 /v1/messages 的下游会全部 401。所以流程反过来 ——
 * 先生成、先复制、二次确认，最后才提交。
 *
 * 提交成功后本地存储自动换成新密钥，当前会话不会被踢出登录。
 */
export function SecuritySection() {
  const confirm = useConfirm()
  const [draft, setDraft] = useState('')
  const [plain, setPlain] = useState(false)
  const [copied, setCopied] = useState(false)
  const [submitting, setSubmitting] = useState(false)

  const copy = async () => {
    if (!draft.trim()) {
      toast.error('先生成或输入密钥再复制')
      return
    }
    try {
      await navigator.clipboard.writeText(draft)
      setCopied(true)
      toast.success('已复制到剪贴板')
    } catch {
      toast.error('复制失败，请手动选中文本')
    }
  }

  const submit = async () => {
    const key = draft.trim()
    if (!key) return
    if (!copied) {
      const ok = await confirm({
        title: '还没复制新密钥',
        description:
          '旧密钥提交后立即失效。新密钥只在这里显示一次，建议先复制保存再继续。',
        confirmText: '仍然继续',
      })
      if (!ok) return
    }
    const ok = await confirm({
      title: '替换登录密钥？',
      description:
        '旧密钥立即失效，使用旧密钥调用 API 的下游都需要换成新密钥。当前浏览器会自动切到新密钥，不会掉线。',
      confirmText: '替换',
      destructive: true,
    })
    if (!ok) return

    setSubmitting(true)
    try {
      await updateAdminKey({ newKey: key })
      storage.setApiKey(key)
      toast.success('登录密钥已替换，本地已切到新密钥')
      setDraft('')
      setPlain(false)
      setCopied(false)
    } catch (err) {
      toast.error('替换失败：' + extractErrorMessage(err))
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <SettingGroup
      title="登录密钥"
      description="用于登录本管理面板，同时是 API 的主密钥（config.json 的 apiKey）"
    >
      <SettingRow
        label="替换密钥"
        hint="旧密钥立即失效。下游客户端需要同步换成新密钥才能继续调用"
      >
        <div className="flex flex-wrap items-center gap-1.5">
          <div className="relative">
            <Input
              type={plain ? 'text' : 'password'}
              value={draft}
              onChange={(e) => {
                setDraft(e.target.value)
                setCopied(false)
              }}
              placeholder="输入或生成新密钥"
              disabled={submitting}
              spellCheck={false}
              autoComplete="new-password"
              className="console-num h-8 w-[min(18rem,55vw)] pr-9 text-[12.5px]"
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
            disabled={submitting}
            onClick={() => {
              setDraft(generateApiKey('sk-admin-'))
              setPlain(true)
              setCopied(false)
            }}
            title="生成一个 32 位随机密钥"
          >
            <Wand2 className="h-3.5 w-3.5" />
            生成
          </Button>
          <Button
            size="sm"
            variant="outline"
            onClick={copy}
            disabled={submitting || !draft.trim()}
          >
            <Copy className="h-3.5 w-3.5" />
            {copied ? '已复制' : '复制'}
          </Button>
          <Button
            size="sm"
            onClick={submit}
            disabled={submitting || !draft.trim()}
          >
            {submitting ? '替换中…' : '替换'}
          </Button>
        </div>
      </SettingRow>
    </SettingGroup>
  )
}
