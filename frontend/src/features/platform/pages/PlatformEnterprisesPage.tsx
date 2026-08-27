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
   Drawer,
   Empty,
} from 'antd';
import { PlusOutlined, SwapOutlined, ReloadOutlined, TeamOutlined } from '@ant-design/icons';
import type { ColumnsType } from 'antd/es/table';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { platformApi } from '../api/platform';
import type { PlatformTenant, CreatePlatformTenantParams } from '../types';
import type { EnterpriseUser } from '../../enterprise/types';

const { Title } = Typography;

const STATUS_META: Record<string, { text: string; color: string }> = {
   ACTIVE: { text: '正常', color: 'green' },
   DISABLED: { text: '已停用', color: 'orange' },
   DELETED: { text: '已删除', color: 'red' },
};

const ROLE_LABEL: Record<string, string> = {
   OWNER: '拥有者',
   ADMIN: '管理员',
   MEMBER: '成员',
};

export const PlatformEnterprisesPage: React.FC = () => {
   const { message } = App.useApp();
   const queryClient = useQueryClient();

   const [page, setPage] = React.useState(1);
   const [size, setSize] = React.useState(20);
   const [keyword, setKeyword] = React.useState('');
   const [statusFilter, setStatusFilter] = React.useState<string | undefined>(undefined);

   const [createModalOpen, setCreateModalOpen] = React.useState(false);
   const [transferTarget, setTransferTarget] = React.useState<PlatformTenant | null>(null);
   const [membersTarget, setMembersTarget] = React.useState<PlatformTenant | null>(null);
   const [createForm] = Form.useForm<CreatePlatformTenantParams>();
   const [transferForm] = Form.useForm<{ target_user_id: string }>();

   const tenantsQuery = useQuery({
      queryKey: ['platform-tenants', page, size, keyword, statusFilter],
      queryFn: async () => {
         const resp = await platformApi.listTenants({
            page,
            size,
            keyword: keyword || undefined,
            status: statusFilter,
         });
         if (resp.code === 200 && resp.data) return resp.data;
         throw new Error(resp.msg || '获取企业列表失败');
      },
   });

   const invalidate = () =>
      queryClient.invalidateQueries({ queryKey: ['platform-tenants'] });

   const currentMemberTenantId = membersTarget?.tenant_id ?? transferTarget?.tenant_id ?? null;

   const membersQuery = useQuery({
      queryKey: ['platform-tenant-members', currentMemberTenantId],
      queryFn: async () => {
         if (!currentMemberTenantId) return [];
         const resp = await platformApi.listTenantMembers(currentMemberTenantId);
         if (resp.code === 200 && resp.data) return resp.data;
         throw new Error(resp.msg || '获取成员列表失败');
      },
      enabled: Boolean(currentMemberTenantId),
   });

   const createMutation = useMutation({
      mutationFn: (data: CreatePlatformTenantParams) => platformApi.createTenant(data),
      onSuccess: (resp) => {
         if (resp.code === 200) {
            message.success('企业创建成功');
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

   const transferMutation = useMutation({
      mutationFn: ({ tenantId, targetUserId }: { tenantId: string; targetUserId: string }) =>
         platformApi.transferOwner(tenantId, targetUserId),
      onSuccess: (resp) => {
         if (resp.code === 200) {
            message.success('所有权已转移');
            setTransferTarget(null);
            transferForm.resetFields();
            invalidate();
         } else {
            message.error(resp.msg || '转移失败');
         }
      },
      onError: (error: unknown) => {
         message.error(errorMessage(error, '转移失败'));
      },
   });

   const lifecycleMutation = useMutation({
      mutationFn: ({ tenantId, action }: { tenantId: string; action: 'disable' | 'restore' | 'delete' }) => {
         if (action === 'disable') return platformApi.disableTenant(tenantId);
         if (action === 'restore') return platformApi.restoreTenant(tenantId);
         return platformApi.deleteTenant(tenantId);
      },
      onSuccess: (resp) => {
         if (resp.code === 200) {
            message.success('操作成功');
            invalidate();
         } else {
            message.error(resp.msg || '操作失败');
         }
      },
      onError: (error: unknown) => {
         message.error(errorMessage(error, '操作失败'));
      },
   });

   const columns: ColumnsType<PlatformTenant> = [
      {
         title: '企业名称',
         dataIndex: 'name',
         key: 'name',
      },
      {
         title: '企业编码',
         dataIndex: 'tenant_code',
         key: 'tenant_code',
         render: (text?: string) => text || '-',
      },
      {
         title: 'OWNER',
         key: 'owner',
         render: (_: unknown, record: PlatformTenant) =>
            record.owner_real_name ? `${record.owner_real_name}（${record.owner_username}）` : record.owner_username || '-',
      },
      {
         title: '成员数',
         dataIndex: 'member_count',
         key: 'member_count',
         width: 90,
         render: (n?: number) => n ?? 0,
      },
      {
         title: '状态',
         dataIndex: 'status',
         key: 'status',
         render: (status: string) => {
            const meta = STATUS_META[status] ?? { text: status, color: 'default' };
            return <Tag color={meta.color}>{meta.text}</Tag>;
         },
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
         width: 320,
         fixed: 'right',
         render: (_: unknown, record: PlatformTenant) => {
            if (record.status === 'DELETED') {
               return <Typography.Text type='secondary'>-</Typography.Text>;
            }
            return (
               <Space size={4}>
                  <Button
                     type='link'
                     size='small'
                     icon={<TeamOutlined />}
                     onClick={() => setMembersTarget(record)}
                  >
                     成员
                  </Button>
                  <Button
                     type='link'
                     size='small'
                     icon={<SwapOutlined />}
                     onClick={() => setTransferTarget(record)}
                  >
                     转移OWNER
                  </Button>
                  {record.status === 'ACTIVE' ? (
                     <Popconfirm
                        title='停用企业'
                        description='停用后该企业成员将无法登录访问。'
                        okText='停用'
                        cancelText='取消'
                        onConfirm={() =>
                           lifecycleMutation.mutate({ tenantId: record.tenant_id, action: 'disable' })
                        }
                     >
                        <Button type='link' size='small' danger>
                           停用
                        </Button>
                     </Popconfirm>
                  ) : (
                     <Button
                        type='link'
                        size='small'
                        icon={<ReloadOutlined />}
                        onClick={() =>
                           lifecycleMutation.mutate({ tenantId: record.tenant_id, action: 'restore' })
                        }
                     >
                        恢复
                     </Button>
                  )}
                  <Popconfirm
                     title='删除企业'
                     description='删除为软删除，请谨慎操作。'
                     okText='删除'
                     cancelText='取消'
                     okButtonProps={{ danger: true }}
                     onConfirm={() =>
                        lifecycleMutation.mutate({ tenantId: record.tenant_id, action: 'delete' })
                     }
                  >
                     <Button type='link' size='small' danger>
                        删除
                     </Button>
                  </Popconfirm>
               </Space>
            );
         },
      },
   ];

   return (
      <div style={{ padding: 24 }}>
         <Card>
            <div
               style={{
                  display: 'flex',
                  justifyContent: 'space-between',
                  alignItems: 'center',
                  marginBottom: 16,
                  flexWrap: 'wrap',
                  gap: 12,
               }}
            >
               <Space direction='vertical' size={0}>
                  <Title level={4} style={{ margin: 0 }}>系统管理 - 企业</Title>
                  <Typography.Text type='secondary'>
                     平台管理员可创建、停用、恢复、删除企业，并转移企业所有权。
                  </Typography.Text>
               </Space>
               <Space>
                  <Input.Search
                     placeholder='搜索企业名称/编码'
                     allowClear
                     onSearch={(value) => {
                        setPage(1);
                        setKeyword(value);
                     }}
                     style={{ width: 220 }}
                  />
                  <Select
                     placeholder='状态'
                     allowClear
                     style={{ width: 120 }}
                     value={statusFilter}
                     onChange={(value) => {
                        setPage(1);
                        setStatusFilter(value);
                     }}
                     options={[
                        { value: 'ACTIVE', label: '正常' },
                        { value: 'DISABLED', label: '已停用' },
                        { value: 'DELETED', label: '已删除' },
                     ]}
                  />
                  <Button
                     type='primary'
                     icon={<PlusOutlined />}
                     onClick={() => setCreateModalOpen(true)}
                  >
                     创建企业
                  </Button>
               </Space>
            </div>

            <Table
               rowKey='tenant_id'
               columns={columns}
               dataSource={tenantsQuery.data?.items || []}
               loading={tenantsQuery.isLoading}
               size='middle'
               scroll={{ x: 'max-content' }}
               pagination={{
                  current: page,
                  pageSize: size,
                  total: tenantsQuery.data?.total || 0,
                  showSizeChanger: true,
                  onChange: (nextPage, nextSize) => {
                     setPage(nextPage);
                     setSize(nextSize);
                  },
                  showTotal: (total) => `共 ${total} 家企业`,
               }}
            />
         </Card>

         {/* 创建企业 */}
         <Modal
            title='创建企业'
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
               initialValues={{ plan_code: 'STANDARD' }}
               onFinish={(values) => createMutation.mutate(values)}
            >
               <Form.Item
                  name='name'
                  label='企业名称'
                  rules={[{ required: true, message: '请输入企业名称' }]}
               >
                  <Input placeholder='企业名称' />
               </Form.Item>
               <Form.Item
                  name='tenant_code'
                  label='企业编码'
                  tooltip='可选；小写字母/数字/_/-，3~64 字符，缺省自动生成'
                  rules={[{ pattern: /^[a-z0-9][a-z0-9_-]{2,63}$/, message: '编码格式不合法' }]}
               >
                  <Input placeholder='例如 acme-bid（可选）' />
               </Form.Item>
               <Form.Item name='plan_code' label='套餐档位'>
                  <Select
                     options={[
                        { value: 'STANDARD', label: '标准版' },
                        { value: 'PRO', label: '专业版' },
                     ]}
                  />
               </Form.Item>
               <Typography.Text type='secondary' style={{ display: 'block', marginBottom: 12 }}>
                  初始 OWNER 账号（企业创建后由该 OWNER 登录管理企业用户）：
               </Typography.Text>
               <Form.Item
                  name='owner_username'
                  label='OWNER 账号'
                  rules={[
                     { required: true, message: '请输入 OWNER 账号' },
                     { min: 3, max: 50, message: '账号长度 3~50 个字符' },
                  ]}
               >
                  <Input placeholder='OWNER 登录账号' autoComplete='off' />
               </Form.Item>
               <Form.Item
                  name='owner_password'
                  label='OWNER 初始密码'
                  rules={[
                     { required: true, message: '请输入初始密码' },
                     { min: 6, max: 100, message: '密码长度 6~100 个字符' },
                  ]}
               >
                  <Input.Password placeholder='初始密码' autoComplete='new-password' />
               </Form.Item>
               <Form.Item name='owner_real_name' label='OWNER 姓名'>
                  <Input placeholder='姓名' />
               </Form.Item>
            </Form>
         </Modal>

         {/* 转移 OWNER：从成员中选择目标 */}
         <Modal
            title={`转移 OWNER — ${transferTarget?.name || ''}`}
            open={!!transferTarget}
            onCancel={() => setTransferTarget(null)}
            onOk={() => transferForm.submit()}
            confirmLoading={transferMutation.isPending}
            okText='转移'
            cancelText='取消'
         >
            <Typography.Paragraph type='secondary'>
               选择一名该企业的活跃成员，作为新的 OWNER（原 OWNER 将降为管理员）。
            </Typography.Paragraph>
            <Form
               form={transferForm}
               layout='vertical'
               onFinish={(values) =>
                  transferTarget &&
                  transferMutation.mutate({
                     tenantId: transferTarget.tenant_id,
                     targetUserId: values.target_user_id,
                  })
               }
            >
               <Form.Item
                  name='target_user_id'
                  label='目标 OWNER'
                  rules={[{ required: true, message: '请选择目标 OWNER' }]}
               >
                  <Select
                     placeholder='选择要接管该企业的成员'
                     loading={membersQuery.isLoading}
                     showSearch
                     optionFilterProp='label'
                     options={(membersQuery.data ?? []).map((m) => ({
                        value: m.user_id,
                        label: `${m.real_name || m.username}（${m.username}｜${ROLE_LABEL[m.role] ?? m.role}）`,
                     }))}
                  />
               </Form.Item>
            </Form>
         </Modal>

         {/* 成员列表 */}
         <Drawer
            title={`成员列表 — ${membersTarget?.name || ''}`}
            open={!!membersTarget}
            onClose={() => setMembersTarget(null)}
            width={640}
         >
            <Table<EnterpriseUser>
               rowKey='user_id'
               size='small'
               loading={membersQuery.isLoading}
               dataSource={membersQuery.data ?? []}
               locale={{ emptyText: <Empty description='暂无成员' /> }}
               pagination={false}
               columns={[
                  { title: '用户 ID', dataIndex: 'user_id', width: 100 },
                  { title: '账号', dataIndex: 'username' },
                  {
                     title: '姓名',
                     dataIndex: 'real_name',
                     render: (t?: string) => t || '-',
                  },
                  {
                     title: '角色',
                     dataIndex: 'role',
                     render: (r: string) => ROLE_LABEL[r] ?? r,
                  },
                  {
                     title: '状态',
                     dataIndex: 'status',
                     render: (s: string) =>
                        s === 'SUSPENDED' ? (
                           <Tag color='orange'>已暂停</Tag>
                        ) : (
                           <Tag color='green'>正常</Tag>
                        ),
                  },
               ]}
            />
         </Drawer>
      </div>
   );
};

function errorMessage(error: unknown, fallback: string): string {
   const record = error as { response?: { data?: { msg?: string } }; message?: string };
   return record?.response?.data?.msg || record?.message || fallback;
}