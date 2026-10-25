// The search outlasts the Files view, so switching views and coming back keeps what you typed.
export const textSearch = $state({
  open: false,
  pattern: '',
  replacement: '',
  showReplace: false,
  matchCase: false,
  wholeWord: false,
  regex: false,
  include: '',
  exclude: '',
  includeIgnored: false,
  showFilters: false,
})
