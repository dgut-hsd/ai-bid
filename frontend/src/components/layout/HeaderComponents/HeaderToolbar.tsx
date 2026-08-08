import React from 'react';
import { Space, Avatar, Button, Dropdown, Spin, type MenuProps } from 'antd';
import { useSelector, useDispatch } from 'react-redux';
import { useNavigate } from 'react-router-dom';
import { Moon, Sun, User, LogOut, Building2, Check } from 'lucide-react';

import type { RootState } from '@/store';
import { logout } from '@/store/slices/authSlice';
import { loginApi } from '@/features/login/api/login';
import { useTenant } from '@/features/tenant/hooks/useTenant';
import { useTheme } from '../../theme-provider';
import { useHeaderStyle } from '../style';

interface HeaderToolbarProps {
   isMobile: boolean;
}

export const HeaderToolbar: React.FC<HeaderToolbarProps> = ({ isMobile }) => {
   const { theme: antdTheme } = useHeaderStyle();
   const dispatch = useDispatch();
   const navigate = useNavigate();
   const { theme, setTheme } = useTheme();
   const { userInfo } = useSelector((state: RootState) => state.auth);

   const {
      tenantList,
      currentTenant,
      currentTenantId,
      isLoading: tenantLoading,
      isSwitching,
      switchTenant,
   } = useTenant();

   const userMenuItems: MenuProps['items'] = [
      {
         key: 'logout',
         icon: <LogOut size={14} />,
         label: '退出登录',
         style: { fontSize: '1.2rem' },
         danger: true,
         onClick: async () => {
            try {
               await loginApi.logout();
            } catch (error) {
               console.error('退出登录失败，请稍后重试', error);
            } finally {
               dispatch(logout());
               navigate('/login', { replace: true });
            }
         },
      },
   ];

   // ── 租户切换下拉菜单项 ────────────────────────────────────────────
   const tenantMenuItems: MenuProps['items'] = [
      ...(tenantList.length > 0
         ? tenantList.map((t) => ({
              key: t.tenant_id,
              icon:
                 t.tenant_id === currentTenantId ? (
                    <Check size={14} />
                 ) : (
                    <Building2 size={14} />
                 ),
              label: (
                 <span
                    style={{
                       fontWeight:
                          t.tenant_id === currentTenantId ? 600 : 400,
                    }}
                 >
                    {t.name}
                    {t.role ? `（${t.role}）` : ''}
                 </span>
              ),
              onClick: () => {
                 if (t.tenant_id !== currentTenantId) {
                    switchTenant(t.tenant_id);
                 }
              },
           }))
         : [
              {
                 key: 'empty',
                 label: '暂无租户',
                 disabled: true,
              },
           ]),
   ];

   return (
      <Space size='small' align='center'>
         {/* ── 租户切换器 ─────────────────────────────────────────── */}
         {isMobile && (
            <Spin size='small' spinning={isSwitching}>
               <Dropdown
                  menu={{ items: tenantMenuItems }}
                  trigger={['hover']}
                  placement='bottomRight'
                  arrow
               >
                  <Button
                     type='text'
                     icon={<Building2 size={16} />}
                     style={{ color: antdTheme.colorTextBase }}
                  />
               </Dropdown>
            </Spin>
         )}

         {!isMobile && (
            <Spin size='small' spinning={isSwitching}>
               <Dropdown
                  menu={{ items: tenantMenuItems }}
                  trigger={['hover']}
                  placement='bottomRight'
                  arrow
               >
                  <Space
                     style={{
                        cursor: 'pointer',
                        padding: '2px 10px',
                        borderRadius: 6,
                        border: `1px solid ${antdTheme.colorBorderSecondary}`,
                        background: antdTheme.colorBgContainer,
                     }}
                  >
                     <Building2
                        size={15}
                        style={{ color: antdTheme.colorPrimary }}
                     />
                     <span
                        style={{
                           fontSize: 13,
                           color: antdTheme.colorTextBase,
                           maxWidth: 140,
                           overflow: 'hidden',
                           textOverflow: 'ellipsis',
                           whiteSpace: 'nowrap',
                        }}
                     >
                        {tenantLoading
                           ? '加载中…'
                           : currentTenant?.name || '未选择租户'}
                     </span>
                  </Space>
               </Dropdown>
            </Spin>
         )}

         {/* ── 主题切换 ───────────────────────────────────────────── */}
         <Button
            type='text'
            icon={theme === 'dark' ? <Sun size={16} /> : <Moon size={16} />}
            onClick={() => setTheme(theme === 'dark' ? 'light' : 'dark')}
            style={{ color: antdTheme.colorTextBase }}
         />

         {/* ── 用户菜单 ───────────────────────────────────────────── */}
         <Dropdown
            menu={{ items: userMenuItems }}
            align={{ offset: [0, -8] }}
            trigger={['hover']}
            placement='bottomRight'
            arrow
         >
            <Space style={{ cursor: 'pointer', padding: '0 4px' }}>
               <Avatar
                  size={isMobile ? 'default' : 'small'}
                  style={{
                     backgroundColor: antdTheme.colorPrimary,
                  }}
               >
                  {userInfo?.realName?.charAt(0) || <User size={14} />}
               </Avatar>

               {!isMobile && (
                  <span
                     style={{
                        fontSize: 12,
                        color: antdTheme.colorTextBase,
                        lineHeight: 1,
                        display: 'inline-flex',
                        alignItems: 'center',
                     }}
                  >
                     {userInfo?.realName || userInfo?.username || '未知用户'}
                  </span>
               )}
            </Space>
         </Dropdown>
      </Space>
   );
};
