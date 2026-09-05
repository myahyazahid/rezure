import { defineStore } from 'pinia'
import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type {
  Attachment,
  AttachmentInfo,
  SubmitTicketPayload,
  TicketCategory,
  TicketHistoryItem,
} from '@/types/support'

const DRAFT_KEY = 'rezure.support.draft'

interface Draft {
  clientTicketId: string
  title: string
  description: string
  category: TicketCategory
  attachments: Attachment[]
  includeSystemInfo: boolean
}

function errorMessage(e: unknown): string {
  if (typeof e === 'string') return e
  if (e instanceof Error) return e.message
  return 'Something went wrong.'
}

function newClientTicketId(): string {
  return crypto.randomUUID()
}

function readDraft(): Draft | null {
  try {
    const raw = localStorage.getItem(DRAFT_KEY)
    return raw ? (JSON.parse(raw) as Draft) : null
  } catch {
    return null
  }
}

export const useSupportStore = defineStore('support', () => {
  const draft = readDraft()

  const clientTicketId = ref(draft?.clientTicketId ?? newClientTicketId())
  const title = ref(draft?.title ?? '')
  const description = ref(draft?.description ?? '')
  const category = ref<TicketCategory>(draft?.category ?? 'bug')
  const attachments = ref<Attachment[]>(draft?.attachments ?? [])
  const includeSystemInfo = ref(draft?.includeSystemInfo ?? true)
  const logText = ref<string | null>(null)

  const attachmentError = ref<string | null>(null)
  const submitting = ref(false)
  const submitError = ref<string | null>(null)
  const submitted = ref(false)

  const history = ref<TicketHistoryItem[]>([])
  const historyError = ref<string | null>(null)
  const loadingHistory = ref(false)

  function persistDraft() {
    const value: Draft = {
      clientTicketId: clientTicketId.value,
      title: title.value,
      description: description.value,
      category: category.value,
      attachments: attachments.value,
      includeSystemInfo: includeSystemInfo.value,
    }
    try {
      localStorage.setItem(DRAFT_KEY, JSON.stringify(value))
    } catch {
      // Best-effort — a full/blocked localStorage shouldn't break the form.
    }
  }

  function clearDraft() {
    try {
      localStorage.removeItem(DRAFT_KEY)
    } catch {
      // Nothing to clean up if it was never written.
    }
  }

  async function addAttachment(path: string) {
    attachmentError.value = null
    if (attachments.value.length >= 5) {
      attachmentError.value = 'Only up to 5 attachments are allowed.'
      return
    }
    try {
      const info = await invoke<AttachmentInfo>('inspect_attachment', { path })
      attachments.value.push({ ...info, path })
      persistDraft()
    } catch (e) {
      attachmentError.value = errorMessage(e)
    }
  }

  function removeAttachment(path: string) {
    attachments.value = attachments.value.filter((a) => a.path !== path)
    persistDraft()
  }

  function setTitle(value: string) {
    title.value = value
    persistDraft()
  }

  function setDescription(value: string) {
    description.value = value
    persistDraft()
  }

  function setCategory(value: TicketCategory) {
    category.value = value
    persistDraft()
  }

  function setIncludeSystemInfo(value: boolean) {
    includeSystemInfo.value = value
    persistDraft()
  }

  function setLogText(value: string | null) {
    logText.value = value
  }

  async function submit() {
    submitting.value = true
    submitError.value = null
    try {
      const payload: SubmitTicketPayload = {
        clientTicketId: clientTicketId.value,
        category: category.value,
        title: title.value,
        description: description.value,
        attachmentPaths: attachments.value.map((a) => a.path),
        includeSystemInfo: includeSystemInfo.value,
        logText: logText.value,
      }
      await invoke('submit_ticket', { payload })
      submitted.value = true
      clearDraft()
      await fetchHistory()
      return true
    } catch (e) {
      // Deliberately keep clientTicketId and every field so Retry resends
      // the exact same logical ticket — the backend dedupes on it.
      submitError.value = errorMessage(e)
      return false
    } finally {
      submitting.value = false
    }
  }

  function startNewTicket() {
    clientTicketId.value = newClientTicketId()
    title.value = ''
    description.value = ''
    category.value = 'bug'
    attachments.value = []
    logText.value = null
    submitted.value = false
    submitError.value = null
    clearDraft()
  }

  async function fetchHistory() {
    loadingHistory.value = true
    try {
      history.value = await invoke<TicketHistoryItem[]>('fetch_ticket_history')
      historyError.value = null
    } catch (e) {
      // Informational only — history is a nice-to-have below the form.
      historyError.value = errorMessage(e)
    } finally {
      loadingHistory.value = false
    }
  }

  return {
    clientTicketId,
    title,
    description,
    category,
    attachments,
    includeSystemInfo,
    logText,
    attachmentError,
    submitting,
    submitError,
    submitted,
    history,
    historyError,
    loadingHistory,
    addAttachment,
    removeAttachment,
    setTitle,
    setDescription,
    setCategory,
    setIncludeSystemInfo,
    setLogText,
    submit,
    startNewTicket,
    fetchHistory,
  }
})
