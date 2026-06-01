import { render } from '@testing-library/react'
import userEvent from '@testing-library/user-event'

/**
 * Renders a component and returns both the render result and userEvent instance.
 * Use this to ensure userEvent is properly set up for each test.
 */
export function renderWithUser(ui: React.ReactElement) {
  return {
    user: userEvent.setup(),
    ...render(ui),
  }
}
