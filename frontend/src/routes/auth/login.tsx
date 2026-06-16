import { createFileRoute, redirect, useNavigate } from '@tanstack/react-router'
import { useState } from 'react'
import { verifyToken } from '@/lib/api-generated/sdk.gen'
import { isAuthenticated, setApiKey } from '@/lib/auth'
import { Button } from '@/components/ui/button'

export const Route = createFileRoute('/auth/login')({
  beforeLoad: () => {
    if (isAuthenticated()) {
      // beforeLoad 守卫：已登录访问登录页直接跳 /admin，避免回退历史死循环。
      throw redirect({ to: '/admin' })
    }
  },
  component: LoginComponent,
})

function LoginComponent() {
  const navigate = useNavigate()
  const [key, setKey] = useState('')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  async function onSubmit(e: React.FormEvent) {
    e.preventDefault()
    setError(null)
    setLoading(true)
    try {
      // 候选 Key 探针：用 per-call auth override（hey-api 在 beforeRequest
      // 会用 options.auth 覆盖全局 _config.auth），不临时写入 localStorage，
      // 避免失败回滚复杂度。setAuthParams → getAuthToken 会自动加 'Bearer ' 前缀。
      const result = await verifyToken({ auth: key })
      if (result.response?.status === 204) {
        setApiKey(key)
        await navigate({ to: '/admin' })
        return
      }
      // 非 204（含 401 无效 Key、5xx）：verifyToken 默认 throwOnError=false，
      // 非 ok 响应时返回 { error, response } 而非抛异常（client.gen.ts:224-234），
      // 故 401/5xx 在此分支处理，不写入 Key，统一提示 "Key 无效"。
      setError('Key 无效')
    } catch {
      // 仅网络异常 / abort（fetch reject，client.gen.ts:90-99）才抛到这里，同样提示无效。
      setError('Key 无效')
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="relative flex min-h-screen flex-col items-center justify-center overflow-hidden px-6">
      <div className="pointer-events-none absolute inset-0 overflow-hidden">
        <div className="absolute -top-40 -right-40 h-96 w-96 rounded-full bg-primary/8 blur-3xl" />
        <div className="absolute -bottom-32 -left-32 h-80 w-80 rounded-full bg-primary/5 blur-3xl" />
      </div>

      <form
        data-testid="login-form"
        onSubmit={onSubmit}
        className="relative z-10 w-full max-w-sm space-y-4 rounded-xl border border-border/50 bg-card/80 p-6 backdrop-blur-sm"
      >
        <div className="space-y-1 text-center">
          <div className="mx-auto mb-2 flex size-9 items-center justify-center rounded-lg bg-primary">
            <span className="font-serif text-sm font-bold text-primary-foreground">R</span>
          </div>
          <h1 className="font-serif text-xl font-semibold tracking-tight">登录 Rwiki</h1>
          <p className="text-xs text-muted-foreground">输入 API Key 以访问文档管理后台</p>
        </div>

        <div className="space-y-1.5">
          <label htmlFor="api-key" className="text-xs font-medium text-muted-foreground">
            API Key
          </label>
          <input
            id="api-key"
            data-testid="api-key-input"
            type="password"
            value={key}
            onChange={(e) => setKey(e.target.value)}
            placeholder="API Key"
            autoComplete="current-password"
            className="h-9 w-full rounded-lg border border-input bg-background px-3 text-sm outline-none transition-colors focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50"
          />
        </div>

        {error && (
          <p data-testid="error-message" role="alert" className="text-xs text-destructive">
            {error}
          </p>
        )}

        <Button
          data-testid="submit-button"
          type="submit"
          size="lg"
          className="w-full"
          disabled={loading || !key}
        >
          {loading ? '校验中…' : '登录'}
        </Button>
      </form>
    </div>
  )
}
