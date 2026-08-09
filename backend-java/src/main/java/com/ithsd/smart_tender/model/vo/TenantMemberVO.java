package com.ithsd.smart_tender.model.vo;

import com.fasterxml.jackson.annotation.JsonProperty;
import lombok.AllArgsConstructor;
import lombok.Builder;
import lombok.Data;
import lombok.NoArgsConstructor;

import java.io.Serializable;
import java.time.LocalDateTime;
import java.util.List;

@Data
@Builder
@NoArgsConstructor
@AllArgsConstructor
public class TenantMemberVO implements Serializable {

    @JsonProperty("member_id")
    private Long memberId;
    @JsonProperty("tenant_id")
    private Long tenantId;
    @JsonProperty("user_id")
    private Long userId;
    private String username;
    @JsonProperty("real_name")
    private String realName;
    private String role;
    private List<String> permissions;
    private String status;
    @JsonProperty("joined_at")
    private LocalDateTime joinedAt;
    @JsonProperty("last_seen_at")
    private LocalDateTime lastSeenAt;
}
