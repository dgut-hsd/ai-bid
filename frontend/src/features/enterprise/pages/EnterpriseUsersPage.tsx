import React from 'react';
import {
   Table,
   Button,
   Modal,
   Form,
   Input,
   Select,
   Tag,
   Space,
   App,
   Card,
   Typography,
   Popconfirm,
   Skeleton,
   Empty,
} from 'antd';
import { PlusOutlined, KeyOutlined, UserDeleteOutlined, EditOutlined } from '@ant-design/icons';
import type { ColumnsType } from 'antd/es/table';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { enterpriseApi } from '../api/enterprise';
import { useStyles } from '../style';
import { EnterpriseUserCard } from '../components/EnterpriseUserCard';
import { useIsMobile } from '@/hooks/useMediaQuery';
import type { EnterpriseUser, EnterpriseRole, CreateUserParams } from '../types';

const { Title } = Typography;

const ROLE_LABEL: Record<string, { text: string; color: string }> = {
   OWNER: { text: '拥有者', color: 'gold' },
   ADMIN: { text: '管理员', color: 'blue' },
   MEMBER: { text: '成员', color: 'default' },
};

const SELECTABLE_ROLES: EnterpriseRole[] = ['ADMIN', 'MEMBER'];

const ROLE_OPTIONS = SELECTABLE_ROLES.map((role) => ({
   value: role,
   label: ROLE_LABEL[role]?.text ?? role,
}));

export const EnterpriseUsersPage: React.FC = () => {
   const { message } = App.useApp();
   const queryClient = useQueryClient();
   const { styles } = useStyles();
   const isMobile = useIsMobile();

   const [createModalOpen, setCreateModalOpen] = React.useState(false);
   const [resetTarget, setResetTarget] = React.useState<EnterpriseUser | null>(null);
   const [editTarget, setEditTarget] = React.useState<EnterpriseUser | null>(null);
   const [createForm] = Form.useForm<CreateUserParams>();
   const [resetForm] = Form.useForm<{ password: string }>();
   const [editForm] = Form.useForm<{ username: string; real_name: string }>();

   const usersQuery = useQuery({
      queryKey: ['enterprise-users'],
      queryFn: async () => {
         const resp = await enterpriseApi.listUsers();
         if (resp.code === 200 && resp.data) return resp.data;
         throw new Error(resp.msg || '获取用户列表失败');
      },
   });

   const invalidate = () =>
      queryClient.invalidateQueries({ queryKey: ['enterprise-users'] });

   const createMutation = useMutation({
      mutationFn: (data: CreateUserParams) => enterpriseApi.createUser(data),
      onSuccess: (resp) => {
         if (resp.code === 200) {
            message.success('用户创建成功');
            setCreateModalOpen(false);
            createForm.resetFields();
            invalidate();
         } else {
            message.error(resp.msg || '创建失败');
         }
      },
      onError: (error: unknown) => {
         message.error(errorMessage(error, '创建失败'));
      },
   });

   const resetMutation = useMutation({
      mutationFn: ({ userId, password }: { userId: string; password: string }) =>
         enterpriseApi.resetPassword(userId, password),
      onSuccess: (resp) => {
         if (resp.code === 200) {
            message.success('密码已重置');
            setResetTarget(null);
            resetForm.resetFields();
         } else {
            message.error(resp.msg || '重置失败');
         }
      },
      onError: (error: unknown) => {
         message.error(errorMessage(error, '重置失败'));
      },
   });

   const removeMutation = useMutation({
      mutationFn: (userId: string) => enterpriseApi.removeMember(userId),
      onSuccess: (resp) => {
         if (resp.code === 200) {
            message.success('成员已移出企业');
            invalidate();
         } else {
            message.error(resp.msg || '移除失败');
         }
      },
      onError: (error: unknown) => {
         message.error(errorMessage(error, '移除失败'));
      },
   });

   const updateUserMutation = useMutation({
      mutationFn: ({
         userId,
         username,
         real_name,
      }: {
         userId: string;
         username: string;
         real_name: string;
      }) => enterpriseApi.updateUser(userId, { username, real_name }),
      onSuccess: (resp) => {
         if (resp.code === 200) {
            message.success('信息已更新');
            setEditTarget(null);
            editForm.resetFields();
            invalidate();
         } else {
            message.error(resp.msg || '更新失败');
         }
      },
      onError: (error: unknown) => {
         message.error(errorMessage(error, '更新失败'));
      },
   });

   const changeRoleMutation = useMutation({
      mutationFn: ({ userId, role }: { userId: string; role: EnterpriseRole }) =>
         enterpriseApi.updateMember(userId, { role }),
      onSuccess: (resp) => {
         if (resp.code === 200) {
            message.success('角色已更新');
            invalidate();
         } else {
            message.error(resp.msg || '角色更新失败');
         }
      },
      onError: (error: unknown) => {
         message.error(errorMessage(error, '角色更新失败'));
      },
   });

   const toggleStatusMutation = useMutation({
      mutationFn: ({ userId, status }: { userId: string; status: 'ACTIVE' | 'SUSPENDED' }) =>
         enterpriseApi.updateMember(userId, { status }),
      onSuccess: (resp) => {
         if (resp.code === 200) {
            message.success('状态已更新');
            invalidate();
         } else {
            message.error(resp.msg || '状态更新失败');
         }
      },
      onError: (error: unknown) => {
         message.error(errorMessage(error, '状态更新失败'));
      },
   });

   const columns: ColumnsType<EnterpriseUser> = [
      {
         title: '账号',
         dataIndex: 'username',
         key: 'username',
      },
      {
         title: '姓名',
         dataIndex: 'real_name',
         key: 'real_name',
         render: (text?: string) => text || '-',
      },
      {
         title: '角色',
         dataIndex: 'role',
         key: 'role',
         render: (role: string, record: EnterpriseUser) => {
            const meta = ROLE_LABEL[role?.toUpperCase()] ?? { text: role || '-', color: 'default' };
            if (!SELECTABLE_ROLES.includes(role?.toUpperCase() as EnterpriseRole)) {
               return <Tag color={meta.color}>{meta.text}</Tag>;
            }
            return (
               <Select
                  size='small'
                  style={{ width: 108 }}
                  value={role}
                  options={ROLE_OPTIONS}
                  disabled={changeRoleMutation.isPending}
                  onChange={(next) =>
                     changeRoleMutation.mutate({
                        userId: record.user_id,
                        role: next as EnterpriseRole,
                     })
                  }
               />
            );
         },
      },
      {
         title: '状态',
         dataIndex: 'status',
         key: 'status',
         render: (status: string) =>
            status === 'SUSPENDED' ? (
               <Tag color='orange'>已暂停</Tag>
            ) : (
               <Tag color='green'>正常</Tag>
            ),
      },
      {
         title: '创建时间',
         dataIndex: 'created_at',
         key: 'created_at',
         responsive: ['md', 'lg', 'xl', 'xxl'],
         render: (t?: string) => (t ? new Date(t).toLocaleString('zh-CN') : '-'),
      },
      {
         title: '操作',
         key: 'action',
         width: 260,
         fixed: 'right',
         render: (_: unknown, record: EnterpriseUser) => {
            const canManage = record.role?.toUpperCase() !== 'OWNER';
            return (
               <Space size={4}>
                  <Button
                     type='link'
                     size='small'
                     icon={<EditOutlined />}
                     onClick={() => {
                        setEditTarget(record);
                        editForm.setFieldsValue({
                           username: record.username,
                           real_name: record.real_name ?? '',
                        });
                     }}
                  >
                     编辑
                  </Button>
                  <Button
                     type='link'
                     size='small'
                     icon={<KeyOutlined />}
                     onClick={() => setResetTarget(record)}
                  >
                     重置密码
                  </Button>
                  {canManage && (
                     <>
                        <Button
                           type='link'
                           size='small'
                           danger={record.status === 'SUSPENDED'}
                           onClick={() =>
                              toggleStatusMutation.mutate({
                                 userId: record.user_id,
                                 status: record.status === 'SUSPENDED' ? 'ACTIVE' : 'SUSPENDED',
                              })
                           }
                        >
                           {record.status === 'SUSPENDED' ? '恢复' : '暂停'}
                        </Button>
                        <Popconfirm
                           title='移出企业'
                           description='该用户将离开本企业，不影响其账号在其他企业的身份。'
                           okText='移出'
                           cancelText='取消'
                           okButtonProps={{ danger: true }}
                           onConfirm={() => removeMutation.mutate(record.user_id)}
                        >
                           <Button type='link' size='small' danger icon={<UserDeleteOutlined />}>
                              移出
                           </Button>
                        </Popconfirm>
                     </>
                  )}
               </Space>
            );
         },
      },
   ];

   return (
      <div style={{ padding: isMobile ? 12 : 24 }}>
         <Card>
            <div
               style={{
                  display: 'flex',
                  justifyContent: 'space-between',
                  alignItems: 'center',
                  marginBottom: 16,
               }}
            >
               <Space direction='vertical' size={0}>
                  <Title level={4} style={{ margin: 0 }}>企业管理 - 用户</Title>
                  <Typography.Text type='secondary'>
                     当前企业的用户账号，由企业 OWNER 创建与管理。
                  </Typography.Text>
               </Space>
               <Button
                  type='primary'
                  icon={<PlusOutlined />}
                  onClick={() => setCreateModalOpen(true)}
               >
                  创建用户
               </Button>
            </div>

            {isMobile ? (
               usersQuery.isLoading && !usersQuery.data ? (
                  <Skeleton active paragraph={{ rows: 4 }} style={{ padding: 12 }} />
               ) : (usersQuery.data?.length ?? 0) === 0 ? (
                  <Empty description='暂无用户' style={{ padding: '32px 0' }} />
               ) : (
                  <div className={styles.mobileCardList}>
                     {(usersQuery.data || []).map((user) => (
                        <EnterpriseUserCard
                           key={user.user_id}
                           user={user}
                           roleUpdating={changeRoleMutation.isPending}
                           onEdit={(u) => {
                              setEditTarget(u);
                              editForm.setFieldsValue({
                                 username: u.username,
                                 real_name: u.real_name ?? '',
                              });
                           }}
                           onResetPassword={setResetTarget}
                           onToggleStatus={(u) =>
                              toggleStatusMutation.mutate({
                                 userId: u.user_id,
                                 status:
                                    u.status === 'SUSPENDED'
                                       ? 'ACTIVE'
                                       : 'SUSPENDED',
                              })
                           }
                           onRemove={(u) => removeMutation.mutate(u.user_id)}
                           onChangeRole={(u, role) =>
                              changeRoleMutation.mutate({
                                 userId: u.user_id,
                                 role,
                              })
                           }
                        />
                     ))}
                  </div>
               )
            ) : (
               <Table
                  rowKey='user_id'
                  columns={columns}
                  dataSource={usersQuery.data || []}
                  loading={usersQuery.isLoading}
                  size='middle'
                  scroll={{ x: 'max-content' }}
                  pagination={false}
               />
            )}
         </Card>

         {/* 创建用户 */}
         <Modal
            title='创建用户'
            open={createModalOpen}
            onCancel={() => setCreateModalOpen(false)}
            onOk={() => createForm.submit()}
            confirmLoading={createMutation.isPending}
            okText='创建'
            cancelText='取消'
         >
            <Form
               form={createForm}
               layout='vertical'
               initialValues={{ role: 'MEMBER' }}
               onFinish={(values) => createMutation.mutate(values)}
            >
               <Form.Item
                  name='username'
                  label='账号'
                  rules={[
                     { required: true, message: '请输入账号' },
                     { min: 3, max: 50, message: '账号长度 3~50 个字符' },
                  ]}
               >
                  <Input placeholder='登录账号（全局唯一）' autoComplete='off' />
               </Form.Item>
               <Form.Item
                  name='password'
                  label='初始密码'
                  rules={[
                     { required: true, message: '请输入初始密码' },
                     { min: 6, max: 100, message: '密码长度 6~100 个字符' },
                  ]}
               >
                  <Input.Password placeholder='初始密码' autoComplete='new-password' />
               </Form.Item>
               <Form.Item
                  name='real_name'
                  label='姓名'
                  rules={[{ required: true, message: '请输入姓名' }]}
               >
                  <Input placeholder='姓名' />
               </Form.Item>
               <Form.Item name='role' label='角色' rules={[{ required: true }]}>
                  <Select options={ROLE_OPTIONS} />
               </Form.Item>
            </Form>
         </Modal>

         {/* 编辑账号/姓名 */}
         <Modal
            title='编辑用户'
            open={!!editTarget}
            onCancel={() => setEditTarget(null)}
            onOk={() => editForm.submit()}
            confirmLoading={updateUserMutation.isPending}
            okText='保存'
            cancelText='取消'
         >
            <Form
               form={editForm}
               layout='vertical'
               onFinish={(values) =>
                  editTarget &&
                  updateUserMutation.mutate({
                     userId: editTarget.user_id,
                     username: values.username,
                     real_name: values.real_name,
                  })
               }
            >
               <Form.Item
                  name='username'
                  label='账号'
                  rules={[
                     { required: true, message: '请输入账号' },
                     { min: 3, max: 50, message: '账号长度 3~50 个字符' },
                  ]}
               >
                  <Input placeholder='账号' autoComplete='off' />
               </Form.Item>
               <Form.Item name='real_name' label='姓名' rules={[{ required: true, message: '请输入姓名' }]}>
                  <Input placeholder='姓名' />
               </Form.Item>
            </Form>
         </Modal>

         {/* 重置密码 */}
         <Modal
            title='重置密码'
            open={!!resetTarget}
            onCancel={() => setResetTarget(null)}
            onOk={() => resetForm.submit()}
            confirmLoading={resetMutation.isPending}
            okText='确定'
            cancelText='取消'
         >
            <Form
               form={resetForm}
               layout='vertical'
               onFinish={(values) =>
                  resetTarget &&
                  resetMutation.mutate({ userId: resetTarget.user_id, password: values.password })
               }
            >
               <Form.Item
                  name='password'
                  label='新密码'
                  rules={[
                     { required: true, message: '请输入新密码' },
                     { min: 6, max: 100, message: '密码长度 6~100 个字符' },
                  ]}
               >
                  <Input.Password placeholder='新密码' autoComplete='new-password' />
               </Form.Item>
            </Form>
         </Modal>
      </div>
   );
};

function errorMessage(error: unknown, fallback: string): string {
   const record = error as { response?: { data?: { msg?: string } }; message?: string };
   return record?.response?.data?.msg || record?.message || fallback;
}