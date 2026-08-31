import { useCallback, useEffect, useState } from 'react'

/**
 * 把筛选态同步进 URL hash 的查询串：`#/traces?status=error&range=60`。
 *
 * 为什么值得做：排查问题时最自然的动作是把当前视图甩给同事 —— "你看这个"。
 * 筛选态只存在组件 state 里的话，链接过去是一个默认视图，对方得照着描述重设一遍
 * 筛选条件。顺带解决刷新页面丢筛选、浏览器后退不回上一组条件。
 *
 * 用 `replaceState` 而非直接改 `location.hash`：
 * - 不触发 hashchange，App 的 Tab 路由不会被误伤
 * - 连续输入搜索词不会往历史栈里塞几十条记录
 *
 * 只写非默认值，URL 保持干净：默认视图的地址就是 `#/traces`。
 */
export function useUrlState<T extends Record<string, string>>(
  tab: string,
  defaults: T,
): [T, (patch: Partial<T>) => void, () => void] {
  const [state, setState] = useState<T>(() => readFromHash(defaults))

  // 浏览器前进 / 后退时跟随 URL 回到对应筛选态
  useEffect(() => {
    const onHash = () => setState(readFromHash(defaults))
    window.addEventListener('hashchange', onHash)
    return () => window.removeEventListener('hashchange', onHash)
    // defaults 是模块级常量，不参与依赖
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const write = useCallback(
    (next: T) => {
      const params = new URLSearchParams()
      for (const [k, v] of Object.entries(next)) {
        // 只跟默认值比较，不额外跳过空串：空串可能本身就是一个有意义的选择。
        // 日志页「不限时间」就是 range=''，而默认是 '1440'——若把空串一并跳过，
        // 这个选择进不了 URL，刷新后会静默弹回 24 小时。
        if (v !== defaults[k]) params.set(k, v)
      }
      const qs = params.toString()
      const url = `${window.location.pathname}${window.location.search}#/${tab}${qs ? `?${qs}` : ''}`
      window.history.replaceState(null, '', url)
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [tab],
  )

  const patch = useCallback(
    (p: Partial<T>) => {
      setState((prev) => {
        const next = { ...prev, ...p }
        write(next)
        return next
      })
    },
    [write],
  )

  const reset = useCallback(() => {
    setState(defaults)
    write(defaults)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [write])

  return [state, patch, reset]
}

function readFromHash<T extends Record<string, string>>(defaults: T): T {
  const hash = window.location.hash
  const qIndex = hash.indexOf('?')
  if (qIndex < 0) return { ...defaults }
  const params = new URLSearchParams(hash.slice(qIndex + 1))
  const out = { ...defaults }
  for (const key of Object.keys(defaults)) {
    const v = params.get(key)
    if (v != null) out[key as keyof T] = v as T[keyof T]
  }
  return out
}

/** 从 hash 中取出 Tab 名，忽略查询串。App 的路由与本 hook 共用这一份解析。 */
export function tabFromHash(): string {
  const raw = window.location.hash.replace(/^#\/?/, '')
  const qIndex = raw.indexOf('?')
  return qIndex >= 0 ? raw.slice(0, qIndex) : raw
}
