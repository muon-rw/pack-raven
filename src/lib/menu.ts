export type MenuAction = { label: string; shortcut?: string; danger?: boolean; disabled?: boolean; run: () => void }
export type Submenu = { label: string; items: MenuItem[] }
export type MenuItem = MenuAction | Submenu | 'separator'
