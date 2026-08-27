package com.ithsd.smart_tender.service;

import com.ithsd.smart_tender.model.dto.EnterpriseCreateUserRequest;
import com.ithsd.smart_tender.model.dto.EnterpriseUpdateMemberRequest;
import com.ithsd.smart_tender.model.dto.EnterpriseUpdateUserRequest;
import com.ithsd.smart_tender.model.vo.EnterpriseUserVO;

import java.util.List;

/** 企业 OWNER 对本企业用户（成员）的管理能力。 */
public interface EnterpriseUserService {

    /** 列出当前企业租户下的所有活跃/暂停成员。 */
    List<EnterpriseUserVO> listUsers();

    /** 创建用户（账号+密码+姓名+角色），同时加入当前企业租户。 */
    EnterpriseUserVO createUser(EnterpriseCreateUserRequest request);

    /** 修改指定用户的账号和/或姓名；账号需全局唯一，变更后旧会话失效。 */
    void updateUser(Long userId, EnterpriseUpdateUserRequest request);

    /** 调整成员角色和/或状态（不能改动 OWNER）。 */
    EnterpriseUserVO updateMember(Long userId, EnterpriseUpdateMemberRequest request);

    /** 重置指定用户的密码，并使该用户的旧会话失效。 */
    void resetPassword(Long userId, String newPassword);

    /** 将成员移出企业：成员关系置 REMOVED，不影响该账号在其他企业的身份。 */
    void removeMember(Long userId);
}