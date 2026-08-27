package com.ithsd.smart_tender.model.vo;

import com.fasterxml.jackson.annotation.JsonProperty;
import lombok.AllArgsConstructor;
import lombok.Builder;
import lombok.Data;
import lombok.NoArgsConstructor;
import java.io.Serializable;

@Data
@Builder
@NoArgsConstructor
@AllArgsConstructor
public class UserInfoVO implements Serializable {
    @JsonProperty("user_id")
    private Long id;
    private String username;
    private String realName;
    /** 是否平台管理员（系统管理者）。用于前端门控「系统管理」入口。 */
    @JsonProperty("is_platform_admin")
    private Boolean isPlatformAdmin;
}
