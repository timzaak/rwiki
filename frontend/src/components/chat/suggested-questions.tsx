interface SuggestedQuestionsProps {
  questions: string[]
  onSelect: (question: string) => void
}

export function SuggestedQuestions({ questions, onSelect }: SuggestedQuestionsProps) {
  if (questions.length === 0) return null

  return (
    <div data-testid="suggested-questions" className="flex flex-wrap gap-1.5 pt-3">
      {questions.map((question, index) => (
        <button
          key={index}
          data-testid="suggested-question-button"
          onClick={() => onSelect(question)}
          className="rounded-full border border-border/50 bg-card/60 px-3 py-1 text-xs text-muted-foreground transition-colors hover:border-primary/30 hover:bg-primary/5 hover:text-foreground"
        >
          {question}
        </button>
      ))}
    </div>
  )
}
