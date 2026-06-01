/* eslint-disable @typescript-eslint/no-empty-object-type */

/**
 * 路由树类型声明
 *
 * 这个文件为 TanStack Router 插件自动生成的 routeTree.gen.ts 提供类型声明。
 * 首次运行 npm run dev 后，插件会生成实际的 routeTree.gen.ts 文件，
 * 之后此声明文件会被自动替代。
 *
 * 如果类型报错，运行 npm run dev 让插件重新生成路由文件。
 */
declare module './routeTree.gen' {
  import type { AnyRoute } from '@tanstack/react-router'

  // eslint-disable-next-line @typescript-eslint/no-empty-interface
  interface RouteTree extends AnyRoute {}
  const routeTree: RouteTree
  export { routeTree }
}
