/**
 * API 客户端配置
 *
 * 这个文件配置了 Axios 实例，用于所有 HTTP 请求。
 *
 * 工作原理：
 * 1. 创建一个 Axios 实例，设置了基础 URL
 * 2. 添加请求拦截器（可用于添加认证 Token）
 * 3. 添加响应拦截器（可用于统一错误处理）
 *
 * 与自动生成的 API 客户端的关系：
 * - openapi-ts 根据 api.json 生成类型安全的 API 函数
 * - 生成的代码使用此处的 Axios 实例发送请求
 * - 生成的代码在 src/lib/api-generated/ 目录下
 *
 * 修改指南：
 * - 修改 API 基础路径 → 修改 baseURL
 * - 添加认证 Token → 在请求拦截器中添加 headers
 * - 统一错误处理 → 在响应拦截器中处理 error
 */
import axios from 'axios'

const apiClient = axios.create({
  // 开发环境通过 Vite proxy 转发到后端
  // 生产环境需要配置实际的 API 地址
  baseURL: '/api',
  withCredentials: true,
  headers: {
    'Content-Type': 'application/json',
  },
})

// 请求拦截器 — 在每个请求发送前执行
apiClient.interceptors.request.use(
  (config) => {
    // 在这里可以添加认证 Token
    // const token = getAuthToken()
    // if (token) {
    //   config.headers.Authorization = `Bearer ${token}`
    // }
    return config
  },
  (error) => {
    return Promise.reject(error)
  }
)

// 响应拦截器 — 在每个响应返回后执行
apiClient.interceptors.response.use(
  (response) => {
    return response
  },
  (error) => {
    // 统一处理 401 未认证错误
    if (error.response?.status === 401) {
      // 跳转到登录页面
      window.location.href = '/auth/login'
    }
    return Promise.reject(error)
  }
)

export default apiClient
