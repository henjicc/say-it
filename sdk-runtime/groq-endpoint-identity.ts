interface GroqEndpointIdentityInput {
  providerId: string
  providerFamilyId?: string
  endpointProfile?: string
  credentialId?: string
  baseUrl?: string
}

interface ResolvedGroqEndpointIdentity {
  providerId: string
  providerFamilyId: string
  endpointProfile?: string
  credentialId: string
  baseUrl?: string
}

function normalizeId(value: string, field: string): string {
  const normalized = value.trim().toLowerCase()
  if (!normalized) throw new Error(`[llm_endpoint_identity_invalid] ${field} must not be empty`)
  return normalized
}

/**
 * Say-It 的内置 Groq bundle 不注册区域端点族。SDK 0.2.4 的通用 chat 内核会静态导入
 * BigModel 区域 profiles；构建时用这个等价的无 profile 分支替换，避免把未选能力带入 QuickJS。
 */
export function resolveLlmEndpointIdentity(
  input: GroqEndpointIdentityInput,
): ResolvedGroqEndpointIdentity {
  const providerId = normalizeId(input.providerId, 'providerId')
  const providerFamilyId = normalizeId(input.providerFamilyId ?? providerId, 'providerFamilyId')
  if (providerFamilyId !== 'groq' || input.endpointProfile !== undefined) {
    throw new Error('[llm_endpoint_profile_unknown] Say-It Groq bundle does not register endpoint profiles')
  }
  return {
    providerId,
    providerFamilyId,
    credentialId: normalizeId(input.credentialId ?? providerId, 'credentialId'),
    baseUrl: input.baseUrl,
  }
}
