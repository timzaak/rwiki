import { createContext, useContext } from 'react'
import type { FeedbackRequest } from '@/lib/api-generated/types.gen'

export type FeedbackSubmitFn = (body: FeedbackRequest) => Promise<void>

export const FeedbackSubmitFnContext = createContext<FeedbackSubmitFn | null>(null)

export function useFeedbackSubmitFnFromContext(): FeedbackSubmitFn | undefined {
  return useContext(FeedbackSubmitFnContext) ?? undefined
}
