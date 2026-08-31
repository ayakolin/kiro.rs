import { toast } from 'sonner'
import { extractErrorMessage } from '@/lib/utils'

/**
 * 各设置分区共用的保存失败提示。
 *
 * 只报错，不报成功 —— 成功由行内那个勾表示（见 console/setting-row.tsx）。
 * 每改一个开关弹一次"已保存"的话，连着调三个参数就是三个 toast 叠在屏幕上，
 * 而它们说的都是用户刚刚亲手做过、且行内已经显示了的事。
 *
 * 单独成文件而不是放在 settings-page.tsx 里：各 section 都要用它，
 * 从 settings-page 反向 import 会与 settings-page → section 形成循环依赖。
 */
export function reportSaveError(err: unknown) {
  toast.error('保存失败：' + extractErrorMessage(err))
}
