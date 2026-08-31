import type { CSSProperties } from 'react'

const FORBIDDEN_VALUE_PATTERN = /url|@import|expression|javascript|[<>]/i
const BLOCKED_PROPERTIES = new Set([
  'position',
  'z-index',
  'inset',
  'top',
  'right',
  'bottom',
  'left',
  'content',
  'display',
  'visibility',
  'pointer-events',
  'transform',
  'animation',
  'transition',
])

export function validateMetadataCss(css: string): string | null {
  const source = css.trim()
  if (!source) return null
  if (source.length > 1000) return '自定义 CSS 不能超过 1000 个字符'
  if (FORBIDDEN_VALUE_PATTERN.test(source)) {
    return '自定义 CSS 不允许外链、脚本表达式或 HTML 标签'
  }

  for (const declaration of source.split(';').map((item) => item.trim()).filter(Boolean)) {
    const colon = declaration.indexOf(':')
    if (colon <= 0 || !declaration.slice(colon + 1).trim()) {
      return `CSS 声明格式不正确：${declaration}`
    }
    const property = declaration.slice(0, colon).trim().toLowerCase()
    if (!/^(--[a-z0-9_-]+|-?[a-z][a-z0-9_-]*)$/.test(property)) {
      return `CSS 属性名不正确：${property}`
    }
    if (BLOCKED_PROPERTIES.has(property)) {
      return `不允许使用可能破坏页面布局的 CSS 属性：${property}`
    }
  }
  return null
}

function toReactProperty(property: string): string {
  if (property.startsWith('--')) return property
  const camel = property.replace(/-([a-z])/g, (_, letter: string) => letter.toUpperCase())
  return camel.startsWith('webkit') ? `W${camel.slice(1)}` : camel
}

export function metadataCssToStyle(css?: string): CSSProperties | undefined {
  if (!css?.trim() || validateMetadataCss(css)) return undefined
  const style: Record<string, string> = {}
  for (const declaration of css.split(';').map((item) => item.trim()).filter(Boolean)) {
    const colon = declaration.indexOf(':')
    const property = declaration.slice(0, colon).trim()
    const value = declaration.slice(colon + 1).trim()
    style[toReactProperty(property)] = value
  }
  return style as CSSProperties
}
