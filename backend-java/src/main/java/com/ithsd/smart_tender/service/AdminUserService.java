package com.ithsd.smart_tender.service;

import com.ithsd.smart_tender.model.dto.AdminCreateUserRequest;
import com.ithsd.smart_tender.model.dto.AdminUpdateUserRequest;
import com.ithsd.smart_tender.model.vo.AdminUserVO;

import java.util.List;

/** 企业管理员对部门用户的管理能力（相同应用内的系统管理模块）。 */
public interface AdminUserService {

    /** 列出当前企业租户下的所有用户。 */
    List<AdminUserVO> listUsers();

    /** 创建用户（账号+密码+姓名+角色），同时加入当前企业租户。 */
    AdminUserVO createUser(AdminCreateUserRequest request);

    /** 修改指定用户的账号和/或姓名；账号需全局唯一，变更后旧会话失效。 */
    void updateUser(Long userId, AdminUpdateUserRequest request);

    /** 重置指定用户的密码，并使该用户的旧会话失效。 */
    void resetPassword(Long userId, String newPassword);

    /** 移除成员：成员关系置 REMOVED，账号停用，旧会话失效。 */
    void removeMember(Long userId);
}