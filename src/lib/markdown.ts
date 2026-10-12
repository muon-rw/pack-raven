import { Marked } from 'marked'

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

export function renderMarkdown(source: string): string {
  return marked.parse(source) as string
}
