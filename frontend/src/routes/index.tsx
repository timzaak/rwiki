/**
 * 主页 `/` —— 重定向至默认频道 `/c/help_center`。
 *
 * `help_center` 由 scripts/demo-start.py 预置数据，其它频道（如 developer_docs）
 * 可能为空；固定跳转正例频道以避免用户误入空态返回 503。
 */
import { createFileRoute, redirect } from '@tanstack/react-router'

const DEFAULT_CHANNEL_ID = 'help_center'

export const Route = createFileRoute('/')({
  beforeLoad: () => {
    throw redirect({
      to: '/c/$channelId',
      params: { channelId: DEFAULT_CHANNEL_ID },
    })
  },
})
