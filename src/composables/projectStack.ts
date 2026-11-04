import type { DatabaseNeed, ProjectStack, RuntimeRequirement } from '@/types/project'

/**
 * Wording for a project's stack and requirements. The facts — what fits,
 * what to pin to — come from Rust (`services::project_stack`); this only
 * phrases them, so the row's tooltip and the dialogs say the same thing.
 */

export type Runtime = 'PHP' | 'Node.js'

/** True when the version serving the project is known not to fit. */
export function runtimeMismatch(
  requirement: RuntimeRequirement | null,
): requirement is RuntimeRequirement {
  return requirement?.fits === false
}

/** "PHP 8.2 or a later version that fits ^8.2" — what to install when
 *  nothing installed fits. */
export function installHint(runtime: Runtime, requirement: RuntimeRequirement): string {
  return requirement.minimumBranch
    ? `${runtime} ${requirement.minimumBranch} or a later version that fits ${requirement.constraint}`
    : `a ${runtime} version that fits ${requirement.constraint}`
}

/** The fix for a runtime that doesn't fit, as one sentence. */
export function runtimeFix(runtime: Runtime, requirement: RuntimeRequirement): string {
  return requirement.suggested
    ? `Pin it to ${runtime} ${requirement.suggested} - the newest installed version that fits.`
    : `None of the installed versions fit. Install ${installHint(runtime, requirement)} on the Switch page, then pin it.`
}

/** What's wrong with a database need, or null when nothing is. A stopped
 *  service isn't wrong: it's started when it's needed. */
export function databaseProblem(db: DatabaseNeed): string | null {
  if (!db.supported) return `Uses ${db.label}, which Rezure doesn't provide - run it yourself.`
  if (db.provider && !db.provider.installed) {
    return `Uses ${db.label}, which isn't installed in Rezure yet.`
  }
  if (db.fits === false) {
    return `Needs ${db.label} ${db.minimum} or newer, but Rezure has ${db.provider?.name} ${db.provider?.version}.`
  }
  return null
}

/** Every problem, one line each — the info icon's tooltip leads with these. */
export function stackProblems(stack: ProjectStack): string[] {
  const problems: string[] = []
  for (const [runtime, requirement] of [
    ['PHP', stack.php],
    ['Node.js', stack.node],
  ] as const) {
    if (runtimeMismatch(requirement)) {
      problems.push(
        `${runtime} ${requirement.serving} doesn't fit ${requirement.constraint} (${requirement.source}).`,
      )
    }
  }
  const pm = stack.packageManager
  if (pm?.fits === false) {
    problems.push(`npm ${pm.current} doesn't fit ${pm.constraint} (package.json engines.npm).`)
  }
  for (const db of stack.databases) {
    const problem = databaseProblem(db)
    if (problem) problems.push(problem)
  }
  return problems
}

/** "Laravel 13.39.0 · PHP ^8.5 · Node.js >=22 · MySQL" — the tooltip when
 *  everything fits. */
export function stackSummary(stack: ProjectStack): string {
  const parts = stack.frameworks
    .slice(0, 3)
    .map((f) => (f.version ? `${f.name} ${f.version}` : f.name))
  if (stack.php) parts.push(`PHP ${stack.php.constraint}`)
  if (stack.node) parts.push(`Node.js ${stack.node.constraint}`)
  if (stack.packageManager) parts.push(stack.packageManager.name)
  for (const db of stack.databases) parts.push(db.label)
  return parts.join(' · ')
}

/** The info icon's tooltip. */
export function stackTooltip(stack: ProjectStack): string {
  const problems = stackProblems(stack)
  return problems.length
    ? `${problems.join('\n')}\nClick for details and fixes.`
    : `${stackSummary(stack)}\nClick for details.`
}

/** How to get a package manager Node.js doesn't bring along. */
export function packageManagerHint(name: string): string | null {
  switch (name) {
    case 'npm':
      return 'Comes with Node.js.'
    case 'pnpm':
    case 'yarn':
      return `Not bundled with Node.js - install it with "npm install -g ${name}", or run "corepack enable" on Node.js 24 and older.`
    case 'bun':
      return 'Installed separately from Node.js - see bun.sh.'
    default:
      return null
  }
}
