package com.ithsd.smart_tender.model.dto;

import lombok.Data;

import java.io.Serializable;

/** 企业管理员调整成员：角色和/或成员状态（两者至少提供一个）。 */
@Data
public class EnterpriseUpdateMemberRequest implements Serializable {

    /** 可选；角色：ADMIN / MEMBER（不允许把成员改为 OWNER）。 */
    private String role;

    /** 可选；成员状态：ACTIVE / SUSPENDED。 */
    private String status;
}