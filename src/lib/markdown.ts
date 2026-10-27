import { Marked } from 'marked'
import { outerArchive } from './viewers'

const escape = (text: string) => text.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`)

const marked = new Marked({
  async: false,
  gfm: true,
  renderer: {
    html({ text }) {
      return escape(text)
    },
    image({ text }) {
      return escape(text)
    },
    link({ href, tokens }) {
      const label = this.parser.parseInline(tokens)
      return /^https?:\/\//i.test(href) ? `<a href="${escape(href)}">${label}</a>` : label
    },
  },
})

export function renderMarkdown(source: string, { headingOffset = 0 } = {}): string {
  const html = marked.parse(source) as string
  if (!headingOffset) return html
  return html.replace(/<(\/?)h([1-6])\b/g, (_, slash, level) => `<${slash}h${Math.min(6, Number(level) + headingOffset)}`)
}

// Files keep their raw HTML here, since READMEs lean on it for centered banners and badges.
// sanitize() decides what of it survives.
const documentMarked = new Marked({ async: false, gfm: true })

export interface Block {
  /** The zero-based source line the block starts on. */
  line: number
  html: string
}

/**
 * Renders a Markdown file in blocks tagged with their source lines, so a preview can follow the
 * editor's scroll position. `imageUrl` returns a loaded URL for a pack-relative image path, or
 * null while it's still loading.
 */
export function renderDocument(source: string, path: string, imageUrl: (path: string) => string | null): Block[] {
  const tokens = documentMarked.lexer(source)
  const context: Context = { path, imageUrl, slugs: new Map() }
  const blocks: Block[] = []
  let line = 0
  for (const token of tokens) {
    if (token.type !== 'space') {
      const html = documentMarked.parser(Object.assign([token], { links: tokens.links }))
      blocks.push({ line, html: sanitize(html, context) })
    }
    line += token.raw.split('\n').length - 1
  }
  return blocks
}

interface Context {
  path: string
  imageUrl: (path: string) => string | null
  slugs: Map<string, number>
  curseforge?: boolean
}

// CurseForge descriptions may use these on top of what READMEs get. We keep their layout styles
// and drop colors, fonts and sizes, which were picked for CurseForge's page and fight our themes.
const CURSEFORGE_TAGS = new Set(['strike', 'q', 'cite', 'acronym', 'section', 'article'])
const CURSEFORGE_STYLES = new Set([
  'font-weight', 'font-style', 'text-align', 'text-indent', 'text-decoration', 'text-transform', 'padding-left', 'border-radius', 'width', 'min-width',
  'display', 'margin-left', 'margin-right', 'float', 'flex', 'flex-wrap', 'flex-direction', 'align-items', 'gap', 'aspect-ratio', 'object-fit',
])
const CURSEFORGE_CLASSES = new Set(['center', 'alignleft', 'aligncenter', 'alignright'])

function keepCurseforgeLayout(node: HTMLElement, style: string | undefined, classes: string | undefined) {
  if (style) {
    const parsed = node.ownerDocument.createElement('span').style
    parsed.cssText = style
    const kept = [...parsed].filter((property) => CURSEFORGE_STYLES.has(property) && !/url\(/i.test(parsed.getPropertyValue(property)))
    if (kept.length) node.setAttribute('style', kept.map((property) => `${property}: ${parsed.getPropertyValue(property)}`).join('; '))
  }
  const keptClasses = (classes ?? '').split(/\s+/).filter((name) => CURSEFORGE_CLASSES.has(name))
  if (keptClasses.length) node.setAttribute('class', keptClasses.join(' '))
}

const ALLOWED_TAGS = new Set(
  'a abbr b blockquote br center code dd del details div dl dt em h1 h2 h3 h4 h5 h6 hr i img input ins kbd li mark ol p pre s small span strong sub summary sup table tbody td tfoot th thead tr u ul'.split(' '),
)
const ALLOWED_ATTRIBUTES = new Set(['align', 'alt', 'title', 'width', 'height', 'colspan', 'rowspan', 'start', 'open', 'checked'])
// We drop these with everything inside them, where other unknown tags only lose the tag.
const DROPPED_TAGS = new Set(['script', 'style', 'iframe', 'object', 'embed', 'form', 'textarea', 'select', 'button', 'template', 'noscript', 'link', 'meta', 'svg', 'math', 'video', 'audio'])

// CurseForge sends mod descriptions as HTML. They have no files of their own, so only web links
// and images survive.
export function sanitizeRemoteHtml(html: string): string {
  return sanitize(html, { path: '', imageUrl: () => null, slugs: new Map(), curseforge: true })
}

// The preview runs inside packraven's own window, so we keep only plain formatting and rewrite
// every link and image ourselves. DOMParser builds the tree without running any of it.
function sanitize(html: string, context: Context) {
  const document = new DOMParser().parseFromString(`<body>${html}</body>`, 'text/html')
  clean(document.body, context)
  return document.body.innerHTML
}

function clean(parent: Element, context: Context) {
  for (const node of [...parent.childNodes]) {
    if (node.nodeType === Node.COMMENT_NODE) {
      node.remove()
      continue
    }
    if (!(node instanceof Element)) continue
    const tag = node.tagName.toLowerCase()
    if (DROPPED_TAGS.has(tag)) {
      node.remove()
      continue
    }
    clean(node, context)
    if (!ALLOWED_TAGS.has(tag) && !(context.curseforge && CURSEFORGE_TAGS.has(tag))) {
      node.replaceWith(...node.childNodes)
      continue
    }
    const original = new Map([...node.attributes].map((a) => [a.name.toLowerCase(), a.value]))
    for (const name of [...node.getAttributeNames()]) {
      if (!ALLOWED_ATTRIBUTES.has(name.toLowerCase())) node.removeAttribute(name)
    }
    if (context.curseforge && node instanceof HTMLElement) keepCurseforgeLayout(node, original.get('style'), original.get('class'))
    const url = (name: string) => (context.curseforge ? curseforgeUrl(original.get(name) ?? '') : (original.get(name) ?? ''))
    if (tag === 'a') rewriteLink(node, url('href'), context)
    else if (tag === 'img') rewriteImage(node, url('src'), context)
    else if (tag === 'input') {
      // Markdown task lists render checkboxes, which only show state here.
      if (original.get('type') === 'checkbox') node.setAttribute('type', 'checkbox')
      else node.remove()
      node.setAttribute('disabled', '')
    } else if (/^h[1-6]$/.test(tag)) {
      node.id = uniqueSlug(node.textContent ?? '', context)
    }
  }
}

// A stray % in a README link or image would make decodeURIComponent throw and break the whole
// preview, so we keep a bad escape as written.
function decodeOrKeep(text: string) {
  try {
    return decodeURIComponent(text)
  } catch {
    return text
  }
}

// Authors can link to CurseForge pages from the site root, and to files without a scheme. Neither
// means anything inside packraven, so we resolve both against CurseForge.
function curseforgeUrl(url: string) {
  if (url.startsWith('//')) return `https:${url}`
  if (url.startsWith('/')) return `https://www.curseforge.com${url}`
  return url
}

function rewriteLink(link: Element, href: string, context: Context) {
  if (/^(https?:|mailto:)/i.test(href)) link.setAttribute('data-external', href)
  else if (href.startsWith('#')) link.setAttribute('data-anchor', decodeOrKeep(href.slice(1)))
  else if (href && !/^[a-z][a-z0-9+.-]*:/i.test(href)) link.setAttribute('data-path', resolve(href, context.path))
  link.setAttribute('href', '#')
}

function rewriteImage(image: Element, src: string, context: Context) {
  image.setAttribute('loading', 'lazy')
  if (/^https:\/\//i.test(src) || /^data:image\//i.test(src)) {
    image.setAttribute('src', src)
  } else if (src && !/^[a-z][a-z0-9+.-]*:/i.test(src)) {
    const path = resolve(src, context.path)
    image.setAttribute('data-path', path)
    const url = context.imageUrl(path)
    if (url) image.setAttribute('src', url)
  } else {
    image.remove()
  }
}

// Like GitHub, a leading slash means the root of the repository, which for a file inside a jar
// is the jar and otherwise the pack.
export function resolve(reference: string, from: string) {
  const clean = decodeOrKeep(reference.split(/[?#]/)[0])
  const archive = outerArchive(from)
  const root = archive === from ? '' : archive
  const base = clean.startsWith('/') ? root : from.slice(0, from.lastIndexOf('/') + 1).replace(/\/$/, '')
  const parts = base ? base.split('/') : []
  for (const part of clean.split('/')) {
    if (part === '..') parts.pop()
    else if (part && part !== '.') parts.push(part)
  }
  return parts.join('/')
}

// GitHub's heading anchors: lowercase, punctuation dropped, spaces as hyphens, and a number
// added to repeats.
function uniqueSlug(text: string, context: Context) {
  const slug = text
    .trim()
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\s_-]/gu, '')
    .replace(/\s/g, '-')
  const seen = context.slugs.get(slug) ?? 0
  context.slugs.set(slug, seen + 1)
  return seen ? `${slug}-${seen}` : slug
}
