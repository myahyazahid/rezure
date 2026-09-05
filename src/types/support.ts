export type TicketCategory = 'bug' | 'feature_request' | 'general'

export interface AttachmentInfo {
  name: string
  sizeBytes: number
}

export interface Attachment extends AttachmentInfo {
  path: string
}

export interface SubmitTicketPayload {
  clientTicketId: string
  category: TicketCategory
  title: string
  description: string
  attachmentPaths: string[]
  includeSystemInfo: boolean
  logText: string | null
}

export type TicketStatus = 'open' | 'in_progress' | 'resolved'

export interface TicketHistoryItem {
  category: TicketCategory
  title: string
  status: TicketStatus
  createdAt: string
}
