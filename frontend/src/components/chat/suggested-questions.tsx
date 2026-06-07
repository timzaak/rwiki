import { Button } from '@/components/ui/button'

interface SuggestedQuestionsProps {
  questions: string[]
  onSelect: (question: string) => void
}

export function SuggestedQuestions({ questions, onSelect }: SuggestedQuestionsProps) {
  if (questions.length === 0) return null

  return (
    <div data-testid="suggested-questions" className="flex flex-col gap-2 px-4">
      {questions.map((question, index) => (
        <Button
          key={index}
          variant="outline"
          className="w-full justify-start text-left text-sm h-auto py-2 px-3"
          data-testid="suggested-question-button"
          onClick={() => onSelect(question)}
        >
          {question}
        </Button>
      ))}
    </div>
  )
}
